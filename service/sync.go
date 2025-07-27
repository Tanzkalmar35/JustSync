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

//  14 func ApplyFileDelta(msg snapshot.WebsocketMessage_FileDelta) error {
//  15 // --- 1. Setup and Configuration ---
//  16 	cfg := utils.GetClientConfig()
//  17 // Construct the absolute path for the file to be modified.
//  18 	absolutePath := filepath.Join(cfg.Session.Path, cfg.Session.Name, msg.FileDelta.Path)
//  19
//  20 // --- 2. Retrieve Old File State from In-Memory Snapshot ---
//  21 // Assumes the existence of a global, in-memory snapshot.
//  22 // A read lock should be acquired before accessing the snapshot to ensure thread safety.
//  23 // globalSnapshot.RLock()
//  24 // defer globalSnapshot.RUnlock()
//  25
//  26 	snapshotFile, ok := globalSnapshot.Files[msg.FileDelta.Path]
//  27 if !ok {
//  28 // This case should ideally not happen in a consistent system.
//  29 // If it does, it means we received a delta for a file we have no prior record of.
//  30 // The safest response is to request a full sync for this file.
//  31 return fmt.Errorf("received delta for untracked file: %s", msg.FileDelta.Path)
//  32 	}
//  33
//  34 // Create a lookup map for old chunk content for fast retrieval.
//  35 // The key is the checksum (as a string for map compatibility) and the value is the raw content.
//  36 	oldChunkContent :=make(map[string][]byte, len(snapshotFile.Chunks))
//  37 for _, chunk := range snapshotFile.Chunks {
//  38 		oldChunkContenstring(chunk.Checksum)] = chunk.Content
//  39 	}
//  40
//  41 // --- 3. Prepare for File Reconstruction ---
//  42 // A temporary struct to hold all chunks (new and moved) that will form the new file.
//  43 type reconstructionChunk struct {
//  44 		Checksum byte
//  45 		Content  byte
//  46 		Offset int64
//  47 	}
//  48
//  49 // Pre-allocate slice capacity to avoid reallocations.
//  50 	chunksForReconstruction :=make([]reconstructionChunk, 0, len(msg.FileDelta.AddedChunks)+len(msg.FileDelta.MovedChunks))
//  51
//  52 // Populate the list with new chunks from the delta message.
//  53 for _, added := range msg.FileDelta.AddedChunks {
//  54 		chunksForReconstruction append(chunksForReconstruction, reconstructionChunk{
//  55 			Checksum: added.Checksum,
//  56 			Content:  added.Content,
//  57 			Offset:   added.NewOffset,
//  58 		})
//  59 	}
//  60
//  61 // Populate the list with moved chunks, retrieving their content from our map.
//  62 for _, moved := range msg.FileDelta.MovedChunks {
//  63 		content, found := oldChunkContenstring(moved.Checksum)]
//  64 if !found {
//  65 // This is a critical error, indicating the delta is corrupt or based on a different
//  66 // file version than our snapshot. The sync cannot proceed.
//  67 return fmt.Errorf("corrupt delta: moved chunk with checksum %x not found", moved.Checksum)
//  68 		}
//  69 		chunksForReconstruction append(chunksForReconstruction, reconstructionChunk{
//  70 			Checksum: moved.Checksum,
//  71 			Content:  content,
//  72 			Offset:   moved.NewOffset,
//  73 		})
//  74 	}
//  75
//  76 // --- 4. Assemble and Verify the New File ---
//  77 // Sort the chunks by their new offset. This is the crucial step that ensures
//  78 // we build the file in the correct order.
//  79 	sort.Slice(chunksForReconstruction,func(i, j int) bool {
//  80 return chunksForReconstruction[i].Offset < chunksForReconstruction[j].Offset
//  81 	})
//  82
//  83 // Use a buffer for efficient in-memory file construction.
//  84 var newFileBuffer bytes.Buffer
//  85 for _, chunk := range chunksForReconstruction {
//  86 		newFileBuffer.Write(chunk.Content)
//  87 	}
//  88 	newFileContent := newFileBuffer.Bytes()
//  89
//  90 // Verify that the reconstructed file's checksum matches the expected checksum from the delta.
//  91 // This guarantees the integrity of the patch operation.
//  92 	hasher := utils.GetHasher()
//  93 	calculatedChecksum := hasher(newFileContent)
//  94 if !bytes.Equal(calculatedChecksum, msg.FileDelta.Checksum) {
//  95 return fmt.Errorf("checksum mismatch after applying delta for %s. Aborting.", msg.FileDelta.Path)
//  96 	}
//  97
//  98 // --- 5. Write to Disk and Update Snapshot ---
//  99 // The new content is verified. Now, write it to the filesystem.
// 100 if err := os.WriteFile(absolutePath, newFileContent, 0644); err != nil {
// 101 return fmt.Errorf("failed to write updated file %s: %w", absolutePath, err)
// 102 	}
// 103
// 104 // Finally, update the in-memory snapshot to reflect the new state of the file.
// 105 // A full write lock must be acquired here to prevent any other reads or writes.
// 106 // globalSnapshot.Lock()
// 107 // defer globalSnapshot.Unlock()
// 108
// 109 	newSnapshotChunks :=make([]*snapshot.InitialSyncChunk, len(chunksForReconstruction))
// 110 for i, chunk := range chunksForReconstruction {
// 111 		newSnapshotChunks[i] = &snapshot.InitialSyncChunk{
// 112 			Checksum: chunk.Checksum,
// 113 			Content:  chunk.Content,
// 114 			Offset:   chunk.Offset,
// 115 			Size:  int64(len(chunk.Content)),
// 116 		}
// 117 	}
// 118
// 119 	globalSnapshot.Files[msg.FileDelta.Path] = &snapshot.InitialSyncFile{
// 120 		Checksum: msg.FileDelta.Checksum,
// 121 		Chunks:   newSnapshotChunks,
// 122 	}
// 123
// 124 	utils.LogInfo"Successfully applied delta to %s", msg.FileDelta.Path)
// 125 return nil
// 126 }
