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

func PrepareInitiateProjectSync() ([]snapshot.WebsocketMessage, error) {
	projectRoot := utils.GetHostConfig().Application.Path
	var messages []snapshot.WebsocketMessage
	// Append start sync msg
	startSyncMsg := snapshot.WebsocketMessage_StartSync{}
	messages = append(messages, snapshot.WebsocketMessage{Payload: &startSyncMsg})

	// Append sync msg's for each file
	if err := filepath.WalkDir(projectRoot, func(absolutePath string, d fs.DirEntry, err error) error {
		if err != nil {
			utils.LogError("Error traversing full project for initial sync: %s", err.Error())
			return err
		}

		// Skip directories
		if d.IsDir() {
			return nil
		}

		file, err := os.Open(absolutePath)
		if err != nil {
			utils.LogError("Error reading file at %s: %s", absolutePath, err.Error())
			return err
		}
		defer file.Close()

		relativePath, err := filepath.Rel(projectRoot, absolutePath)
		if err != nil {
			utils.LogError("Could not shrink the absolute path to be relative due to: %s", err.Error())
			return err
		}

		fileChunks, err := utils.ChunkFileContentDefined(file)
		if err != nil {
			utils.LogError("Could not chunk content of file %s due to error: %s", absolutePath, err.Error())
			return err
		}

		fileContent, err := io.ReadAll(file)
		if err != nil {
			utils.LogError("Could not read content of file %s due to error: %s", absolutePath, err.Error())
			return err
		}

		fileSync := &snapshot.InitialSyncFile{
			Checksum: utils.GetHasher()(fileContent),
			Chunks:   fileChunks,
		}
		syncMsg := snapshot.WebsocketMessage{
			Payload: &snapshot.WebsocketMessage_InitialFile{
				InitialFile: &snapshot.InitialSyncFileWithPath{
					Path: []byte(relativePath),
					File: fileSync,
				},
			},
		}
		messages = append(messages, syncMsg)

		return nil
	}); err != nil {
		utils.LogError("Error traversing full project for initial sync: %s", err.Error())
		return messages, err
	}

	// Append end sync msg
	endSyncMsg := snapshot.WebsocketMessage{
		Payload: &snapshot.WebsocketMessage_EndSync{},
	}
	messages = append(messages, endSyncMsg)

	return messages, nil
}

func PrepareReceiveProjectSync() error {
	cfg := utils.GetClientConfig()
	path := cfg.Session.Path + cfg.Session.Name

	_, err := os.Stat(cfg.Session.Path)
	if err == nil {
		utils.LogError("Folder with name %s already existing at %s", cfg.Session.Name, cfg.Session.Path)
		return err
	}
	if !errors.Is(err, fs.ErrNotExist) {
		utils.LogError("Something went wrong validating project path: %s", err.Error())
		return err
	}

	if err := os.Mkdir(path, 0755); err != nil {
		utils.LogError("Could not create directory %s at %s", cfg.Session.Name, cfg.Session.Path)
		return err
	}

	return nil
}

// ProcessNewFileSync builds up a file at a given path and fills it with the desired content
func ProcessNewFileSync(msg snapshot.WebsocketMessage_InitialFile) error {
	// Build the path for the new file
	cfg := utils.GetClientConfig()
	path := filepath.Join(cfg.Session.Path, cfg.Session.Name, string(msg.InitialFile.Path))
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0755); err != nil {
		utils.LogError("Unable to create directory structure '%s' due to: %s", dir, err.Error())
		return err
	}

	// Create the actual file
	file, err := os.Create(path)
	if err != nil {
		utils.LogError("Could not create file %s due to error: %s", path, err.Error())
		return err
	}

	// Fill the file with the actual content
	totalWrittenBytes := 0
	for _, chunk := range msg.InitialFile.File.Chunks {
		b, err := file.WriteAt(chunk.Content, chunk.Offset)
		if err != nil {
			utils.LogError("Could not write content to file at '%s' due to: %s", msg.InitialFile.Path, err.Error())
			return err
		}
		totalWrittenBytes += b
		utils.LogDebug("Wrote chunk of size %s to file %s", b, msg.InitialFile.Path)
	}

	// Check content checksum
	utils.LogInfo("Wrote %b bytes to %s", totalWrittenBytes, msg.InitialFile.Path)
	return nil
}

// ApplyFileDelta reconstructs a file based on a delta message.
func ApplyFileDelta(msg snapshot.WebsocketMessage_FileDelta) error {
	oldSnapshotFile, ok := snapshot.GetSnapshot().Files[msg.FileDelta.Path]
	if !ok {
		// File does not appear in local register, must have been added by remote
		if err := applyNewFileSync(msg); err != nil {
			utils.LogError(err.Error())
			return err
		}
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

	// Populate the list with new chunks from the delta message.
	for _, added := range msg.FileDelta.AddedChunks {
		chunksForReconstruction = append(chunksForReconstruction, reconstructionChunk{
			Checksum: added.Checksum,
			Content:  added.Content,
			Offset:   added.NewOffset,
		})
	}

	// Populate the list with moved chunks, retrieving their content from our map.
	for _, moved := range msg.FileDelta.MovedChunks {
		content, found := oldChunkMap[[32]byte(moved.Checksum)]
		if !found {
			err := "Chunk with checksum '%s' was supposed to be moved, but was not found locally"
			utils.LogError(err, moved.Checksum)
			return fmt.Errorf(err, moved.Checksum)
		}

		chunksForReconstruction = append(chunksForReconstruction, reconstructionChunk{
			Checksum: moved.Checksum,
			Content:  content,
			Offset:   moved.NewOffset,
		})
	}

	// Sort them according to their offset
	sort.Slice(chunksForReconstruction, func(i, j int) bool {
		return chunksForReconstruction[i].Offset < chunksForReconstruction[j].Offset
	})

	// Use a buffer for efficient in-memory file construction.
	var newFileBuffer bytes.Buffer
	for _, chunk := range chunksForReconstruction {
		newFileBuffer.Write(chunk.Content)
	}
	newFileContent := newFileBuffer.Bytes()

	// Verify that the reconstructed file's checksum matches the expected checksum from the delta.
	// This guarantees the integrity of the patch operation.
	hasher := utils.GetHasher()
	calculatedChecksum := hasher(newFileContent)
	if !bytes.Equal(calculatedChecksum, msg.FileDelta.Checksum) {
		return fmt.Errorf("checksum mismatch after applying delta for %s. Aborting.", msg.FileDelta.Path)
	}

	// --- 5. Write to Disk and Update Snapshot ---
	// The new content is verified. Now, write it to the filesystem.
	if err := os.WriteFile(msg.FileDelta.Path, newFileContent, 0644); err != nil {
		return fmt.Errorf("failed to write updated file %s: %w", msg.FileDelta.Path, err)
	}

	// Finally, update the in-memory snapshot to reflect the new state of the file.
	// A full write lock must be acquired here to prevent any other reads or writes.
	newSnapshotChunks := make([]*snapshot.InitialSyncChunk, len(chunksForReconstruction))
	for i, chunk := range chunksForReconstruction {
		newSnapshotChunks[i] = &snapshot.InitialSyncChunk{
			Checksum: chunk.Checksum,
			Content:  chunk.Content,
			Size:     int64(len(chunk.Content)),
		}
	}

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
	if len(msg.FileDelta.MovedChunks) != 0 || len(msg.FileDelta.RemovedChunkHashes) != 0 {
		// We received a file delta, that we don't have any record of existing.
		// This should not happen.
		err := "File delta received for file that locally does not exist... File: %s"
		utils.LogError(err, msg.FileDelta.Path)
		return fmt.Errorf(err, msg.FileDelta.Path)
	}

	// A valid new file was created, copy that
	file, err := os.Create(msg.FileDelta.Path)
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
