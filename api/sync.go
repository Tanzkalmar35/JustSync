package api

import (
	"JustSync/snapshot"
	"JustSync/utils"
	socket "JustSync/websocket"
	"bytes"
	"encoding/json"
	"net/http"
	"os"

	"github.com/gorilla/websocket"
	"google.golang.org/protobuf/proto"
)

func RequestSync(w http.ResponseWriter, r *http.Request) {
	utils.LogInfo("Sync requested")

	// Receive message content
	var body struct {
		Path string `json:"path"`
	}
	if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		utils.LogError("Invalid json body data provided")
		return
	}

	// Open the file for chunking.
	file, err := os.Open(body.Path)
	if err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		utils.LogError("Could not read file data: %s", err.Error())
		return
	}
	defer file.Close()

	// Immediately chunk the file to get the definitive list of new chunks.
	newChunks, err := utils.ChunkFileContentDefined(file)
	if err != nil {
		utils.LogError("An error while chunking file '%s': %s", body.Path, err.Error())
		http.Error(w, "Failed to chunk file", http.StatusInternalServerError)
		return
	}

	// Reconstruct the file content FROM THE CHUNKS to get the definitive content.
	var finalSize int64
	for _, chunk := range newChunks {
		chunkEnd := chunk.Offset + int64(len(chunk.Content))
		if chunkEnd > finalSize {
			finalSize = chunkEnd
		}
	}
	reconstructedContent := make([]byte, finalSize)
	for _, chunk := range newChunks {
		copy(reconstructedContent[chunk.Offset:], chunk.Content)
	}

	// Calculate the checksum on this reconstructed content. This is the authoritative hash.
	hasher := utils.GetHasher()
	hash := hasher(reconstructedContent)

	// Now, check if the file has actually changed.
	snap := snapshot.GetSnapshot()
	if oldFile, ok := snap.Files[body.Path]; ok {
		if bytes.Equal(hash, oldFile.Checksum) {
			utils.LogInfo("Sync request rejected, no change in file detected.")
			w.WriteHeader(http.StatusOK) // Still a success, just no action needed.
			return
		}
	}

	// Prepare new snapshot object
	newSnapshot := snapshot.GetSnapshot()
	// Ensure the file entry exists in the snapshot before trying to access its chunks
	if _, ok := newSnapshot.Files[body.Path]; !ok {
		newSnapshot.Files[body.Path] = &snapshot.InitialSyncFile{}
	}
	newSnapshot.Files[body.Path].Checksum = hash
	newSnapshot.Files[body.Path].Chunks = newChunks // Replace old chunks with the new definitive ones

	// Prepare file delta calculation
	oldChunkMap := make(map[string]*snapshot.InitialSyncChunk) // hash -> Chunk
	newChunkMap := make(map[string]*snapshot.InitialSyncChunk) // hash -> Chunk
	// Use the old snapshot for comparison
	if oldFile, ok := snap.Files[body.Path]; ok {
		for _, chunk := range oldFile.Chunks {
			oldChunkMap[string(chunk.Checksum)] = chunk
		}
	}
	for _, chunk := range newChunks {
		newChunkMap[string(chunk.Checksum)] = chunk
	}

	snapshot.WriteSnapshot(newSnapshot)

	msg := snapshot.FileDelta{
		Path:               body.Path,
		Checksum:           hash, // Use the authoritative hash
		AddedChunks:        []*snapshot.AddedChunk{},
		MovedChunks:        []*snapshot.MovedChunk{},
		RemovedChunkHashes: [][]byte{},
	}

	for newChunkHash, newChunk := range newChunkMap {
		if oldChunk, exists := oldChunkMap[newChunkHash]; !exists {
			// Chunk added
			msg.AddedChunks = append(msg.AddedChunks, &snapshot.AddedChunk{
				Checksum:  newChunk.Checksum,
				Content:   newChunk.Content,
				NewOffset: newChunk.Offset,
			})
		} else if oldChunk.Offset != newChunk.Offset {
			// Chunk moved
			msg.MovedChunks = append(msg.MovedChunks, &snapshot.MovedChunk{
				Checksum:  newChunk.Checksum,
				NewOffset: newChunk.Offset,
			})
		}
	}

	for oldChunkHash := range oldChunkMap {
		if _, exists := newChunkMap[oldChunkHash]; !exists {
			msg.RemovedChunkHashes = append(msg.RemovedChunkHashes, []byte(oldChunkHash))
		}
	}

	msgWrapper := snapshot.WebsocketMessage{
		Payload: &snapshot.WebsocketMessage_FileDelta{
			FileDelta: &msg,
		},
	}
	msgBytes, err := proto.Marshal(&msgWrapper)
	if err != nil {
		utils.LogError("Invalid msg constructed, could not sync file %s. Error: %s", msg.Path, err.Error())
		return
	}
	if err := socket.GetHostConnection().WriteMessage(websocket.TextMessage, msgBytes); err != nil {
		utils.LogError("Failed to send sync message to host: %s", err.Error())
	}

	w.WriteHeader(http.StatusOK)

	utils.LogInfo("Sync accepted and sent to host")
}
