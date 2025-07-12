package api

import (
	"JustSync/snapshot"
	"JustSync/utils"
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
)

func RequestSync(w http.ResponseWriter, r *http.Request) {
	utils.LogInfo("Sync requested")

	// Receive message content
	var body struct{ path string }
	if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		utils.LogError("Invalid json body data provided")
		return
	}

	// Getting file content
	file, err := os.Open(body.path)
	if err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		utils.LogError("Could not read file data: %s", err.Error())
		return
	}
	defer file.Close()

	content, err := io.ReadAll(file)
	if err != nil {
		errMsg := "An error occurred attempting to read file %s: %s"
		utils.LogError(errMsg, file, err.Error())
		w.WriteHeader(http.StatusBadRequest)
		fmt.Print(w, errMsg, file, err.Error())
	}

	// Reading snapshot
	hasher := utils.GetHasher()
	hash := hasher(content)
	snap, err := snapshot.ReadSnapshot("snapshot/SNAPSHOT.sync.snap")
	if err != nil {
		http.Error(w, err.Error(), http.StatusNotAcceptable)
		utils.LogError("Snapshot not found or corrupted, maybe restart the session?: %s", err.Error())
		return
	}

	// Checking if changes were made
	if bytes.Equal(hash, snap.Files[body.path].WholeHash) {
		utils.LogInfo("Sync request rejected, no change in file detected.")
		return
	}

	// Chunk new file content
	newChunks, err := utils.ChunkFileContentDefined(file)
	if err != nil {
		utils.LogError("An error while chunking file '%s': %s", body.path, err.Error())
		return
	}

	// Implement chunk changes thingy
	// compareWithSnapshot detects changes from previous state
	// func compareWithSnapshot(current *FileManifest, previous *FileManifest, chunkData []ChunkMetadata) *SyncOperation {
	// 	op := &SyncOperation{
	// 		NewChunks:  make(map[string][]byte),
	// 		Manifest:   *current,
	// 		TotalBytes: 0,
	// 	}
	//
	// 	// Create lookup for existing chunks on server
	// 	existingSet := make(map[string]bool)
	// 	if previous != nil {
	// 		for _, hash := range previous.Chunks {
	// 			existingSet[hash] = true
	// 		}
	// 	}
	//
	// 	// Identify new chunks and prepare for transfer
	// 	for _, meta := range chunkData {
	// 		if !existingSet[meta.Hash] {
	// 			op.TotalBytes += int64(meta.Size)
	// 		} else {
	// 			op.ExistingBytes += int64(meta.Size)
	// 		}
	// 	}
	//
	// 	return op
	// }

	// TODO: Sync file chunks
	// websocket.GetHub().Broadcast <-
	w.WriteHeader(http.StatusOK)
}

func HeartBeat(w http.ResponseWriter, r *http.Request) {
	utils.LogInfo("Heartbeat received")

	w.WriteHeader(http.StatusOK)
	fmt.Fprintln(w, "Heartbeat successful")
}
