package service

import (
	"JustSync/snapshot"
	"JustSync/utils"
	"bytes"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"sort"
)

// ApplyFileDelta reconstructs a file based on a delta message.
func ApplyFileDelta(msg snapshot.WebsocketMessage_FileDelta) error {
	cfg := utils.GetClientConfig()
	absolutePath := filepath.Join(cfg.Session.Path, cfg.Session.Name, msg.FileDelta.Path)

	oldSnapshotFile, ok := snapshot.GetSnapshot().Files[msg.FileDelta.Path]
	if !ok {
		// File does not appear in local register, must have been added by remote
		if err := applyNewFileSync(msg); err != nil {
			utils.LogError(err.Error())
			return err
		}
		return nil
	}

	if bytes.Equal(oldSnapshotFile.Checksum, msg.FileDelta.Checksum) {
		// The broadcasted sync was originally from this client. Therefore, just ignore the patch.
		// TODO: Edit broadcasting to avoid unnecessary network round trip
		return nil
	}

	oldChunkMap := make(map[[32]byte][]byte)
	for _, chunk := range oldSnapshotFile.Chunks {
		var checksum [32]byte
		copy(checksum[:], chunk.Checksum)
		oldChunkMap[checksum] = chunk.Content
	}

	type reconstructionChunk struct {
		Checksum []byte
		Content  []byte
		Offset   int64
	}

	// Pre-allocate slice capacity to avoid reallocations.
	chunksForReconstruction := make([]reconstructionChunk, 0, len(msg.FileDelta.AddedChunks)+len(msg.FileDelta.MovedChunks))
	var finalSize int64 = 0

	// Populate the list with new chunks from the delta message.
	for _, added := range msg.FileDelta.AddedChunks {
		chunk := reconstructionChunk{
			Checksum: added.Checksum,
			Content:  added.Content,
			Offset:   added.NewOffset,
		}
		chunksForReconstruction = append(chunksForReconstruction, chunk)
		chunkEnd := chunk.Offset + int64(len(chunk.Content))
		if chunkEnd > finalSize {
			finalSize = chunkEnd
		}
	}

	// Populate the list with moved chunks, retrieving their content from our map.
	for _, moved := range msg.FileDelta.MovedChunks {
		content, found := oldChunkMap[[32]byte(moved.Checksum)]
		if !found {
			err := "Chunk with checksum '%s' was supposed to be moved, but was not found locally"
			utils.LogError(err, moved.Checksum)
			return fmt.Errorf(err, moved.Checksum)
		}

		chunk := reconstructionChunk{
			Checksum: moved.Checksum,
			Content:  content,
			Offset:   moved.NewOffset,
		}
		chunksForReconstruction = append(chunksForReconstruction, chunk)
		chunkEnd := chunk.Offset + int64(len(chunk.Content))
		if chunkEnd > finalSize {
			finalSize = chunkEnd
		}
	}

	// Identify unchanged chunks and add them to the reconstruction list.
	// An unchanged chunk is one that exists in the old snapshot and is not marked as moved or removed.
	movedOrRemovedHashes := make(map[string]bool)
	for _, moved := range msg.FileDelta.MovedChunks {
		movedOrRemovedHashes[string(moved.Checksum)] = true
	}
	for _, removed := range msg.FileDelta.RemovedChunkHashes {
		movedOrRemovedHashes[string(removed)] = true
	}

	for _, oldChunk := range oldSnapshotFile.Chunks {
		if _, isMovedOrRemoved := movedOrRemovedHashes[string(oldChunk.Checksum)]; !isMovedOrRemoved {
			// This chunk is unchanged, add it to the reconstruction list.
			chunk := reconstructionChunk{
				Checksum: oldChunk.Checksum,
				Content:  oldChunk.Content,
				Offset:   oldChunk.Offset, // It keeps its original offset
			}
			chunksForReconstruction = append(chunksForReconstruction, chunk)
			// Also update finalSize with this chunk
			chunkEnd := chunk.Offset + int64(len(chunk.Content))
			if chunkEnd > finalSize {
				finalSize = chunkEnd
			}
		}
	}

	// Sort chunks by offset before reconstruction
	sort.Slice(chunksForReconstruction, func(i, j int) bool {
		return chunksForReconstruction[i].Offset < chunksForReconstruction[j].Offset
	})
	newFileContent := make([]byte, finalSize)

	// Sort them according to their offset
	for _, chunk := range chunksForReconstruction {
		copy(newFileContent[chunk.Offset:], chunk.Content)
	}

	// Verify that the reconstructed file's checksum matches the expected checksum from the delta.
	// This guarantees the integrity of the patch operation.
	hasher := utils.GetHasher()
	calculatedChecksum := hasher(newFileContent)

	// --- START DEBUG LOGGING ---
	utils.LogDebug("------ Applying Delta for: %s ------", msg.FileDelta.Path)
	utils.LogDebug("Final calculated size: %d bytes", finalSize)
	utils.LogDebug("Number of chunks for reconstruction: %d", len(chunksForReconstruction))
	utils.LogDebug("Expected checksum: %x", msg.FileDelta.Checksum)
	utils.LogDebug("Calculated checksum: %x", calculatedChecksum)
	utils.LogDebug("Checksums are equal: %t", bytes.Equal(calculatedChecksum, msg.FileDelta.Checksum))
	utils.LogDebug("-------------------------------------------------")
	// --- END DEBUG LOGGING ---

	if !bytes.Equal(calculatedChecksum, msg.FileDelta.Checksum) {
		return fmt.Errorf("checksum mismatch after applying delta for %s. Aborting.", msg.FileDelta.Path)
	}

	// The new content is verified. Now, write it to the filesystem.
	if err := os.WriteFile(absolutePath, newFileContent, 0644); err != nil {
		return fmt.Errorf("failed to write updated file %s: %w", msg.FileDelta.Path, err)
	}

	// Finally, update the in-memory snapshot to reflect the new state of the file.
	// A full write lock must be acquired here to prevent any other reads or writes.
	newSnapshotChunks := make([]*snapshot.InitialSyncChunk, len(chunksForReconstruction))
	for i, chunk := range chunksForReconstruction {
		newSnapshotChunks[i] = &snapshot.InitialSyncChunk{
			Checksum: chunk.Checksum,
			Content:  chunk.Content,
			Offset:   chunk.Offset,
			Size:     int64(len(chunk.Content)),
		}
	}

	sort.Slice(newSnapshotChunks, func(i, j int) bool {
		return newSnapshotChunks[i].Offset < newSnapshotChunks[j].Offset
	})

	// Write new snapshot
	oldSnapshot := snapshot.GetSnapshot()
	oldSnapshot.Files[msg.FileDelta.Path] = &snapshot.InitialSyncFile{
		Checksum: msg.FileDelta.Checksum,
		Chunks:   newSnapshotChunks,
	}
	snapshot.WriteSnapshot(oldSnapshot)

	utils.LogInfo("Successfully applied delta to %s", msg.FileDelta.Path)
	return nil
}

// applyNewFileSync applies sync requests containing new files that the local register does not have.
func applyNewFileSync(msg snapshot.WebsocketMessage_FileDelta) error {
	cfg := utils.GetClientConfig()
	absolutePath := filepath.Join(cfg.Session.Path, cfg.Session.Name, msg.FileDelta.Path)

	if len(msg.FileDelta.MovedChunks) != 0 || len(msg.FileDelta.RemovedChunkHashes) != 0 {
		// We received a file delta, that we don't have any record of existing.
		// This should not happen.
		err := "File delta received for file that locally does not exist... File: %s"
		utils.LogError(err, msg.FileDelta.Path)
		return fmt.Errorf(err, msg.FileDelta.Path)
	}

	// A valid new file was created, copy that
	file, err := os.Create(absolutePath)
	if err != nil {
		utils.LogError("Could not create file %s due to error: %s", msg.FileDelta.Path, err.Error())
		return err
	}

	// Fill new file with content
	var newChunks []*snapshot.InitialSyncChunk
	for _, chunk := range msg.FileDelta.AddedChunks {
		file.WriteAt(chunk.Content, chunk.NewOffset)

		// Prepare for snapshot
		snapshotChunk := snapshot.InitialSyncChunk{
			Checksum: chunk.Checksum,
			Content:  chunk.Content,
			Offset:   chunk.NewOffset,
		}
		newChunks = append(newChunks, &snapshotChunk)
	}

	// Update snapshot
	newFileContent, err := io.ReadAll(file)
	if err != nil {
		utils.LogError("Error retrieving content of file that was just created via sync request. File: '%s', Error: %s", msg.FileDelta.Path, err)
		return err
	}

	snapshotFile := snapshot.InitialSyncFile{
		Checksum: utils.GetHasher()(newFileContent),
		Chunks:   newChunks,
	}
	oldSnapshot := snapshot.GetSnapshot()
	oldSnapshot.Files[msg.FileDelta.Path] = &snapshotFile

	return nil
}
