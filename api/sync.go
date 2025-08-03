package api

import (
	"JustSync/snapshot"
	"JustSync/utils"
	socket "JustSync/websocket"
	"bytes"
	"encoding/json"
	"net/http"
	"os"
	"path/filepath"

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
	fileContent, err := os.ReadFile(body.Path)
	if err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		utils.LogError("Could not read file data: %s", err.Error())
		return
	}

	// Convert the absolute path to a relative path against the project root.
	cfg := utils.GetClientConfig()
	projectRoot := filepath.Join(cfg.Session.Path, cfg.Session.Name)
	relativePath, err := filepath.Rel(projectRoot, body.Path)
	if err != nil {
		http.Error(w, "Failed to create relative path", http.StatusInternalServerError)
		utils.LogError("Could not make path '%s' relative to '%s': %s", body.Path, projectRoot, err.Error())
		return
	}

	// Immediately chunk the file to get the definitive list of new chunks.
	reader := bytes.NewReader(fileContent)
	newChunks, err := utils.ChunkFileContentDefined(reader)
	if err != nil {
		utils.LogError("An error while chunking file '%s': %s", body.Path, err.Error())
		http.Error(w, "Failed to chunk file", http.StatusInternalServerError)
		return
	}

	// Calculate the checksum on this reconstructed content. This is the authoritative hash.
	hasher := utils.GetHasher()
	hash := hasher(fileContent)

	// Now, check if the file has actually changed.
	snap := snapshot.GetSnapshot()
	if oldFile, ok := snap.Files[relativePath]; ok {
		if bytes.Equal(hash, oldFile.Checksum) {
			utils.LogInfo("Sync request rejected, no change in file detected.")
			w.WriteHeader(http.StatusOK) // Still a success, just no action needed.
			return
		}
	}

	// Prepare file delta calculation
	oldChunkMap := make(map[string]*snapshot.InitialSyncChunk) // hash -> Chunk
	newChunkMap := make(map[string]*snapshot.InitialSyncChunk) // hash -> Chunk

	// Populate the old chunk map from the unmodified snapshot.
	if oldFile, ok := snap.Files[relativePath]; ok {
		for _, chunk := range oldFile.Chunks {
			oldChunkMap[string(chunk.Checksum)] = chunk
		}
	}
	// Populate the new chunk map from the chunks we just generated.
	for _, chunk := range newChunks {
		newChunkMap[string(chunk.Checksum)] = chunk
	}

	msg := snapshot.FileDelta{
		Path:               relativePath,
		Checksum:           hash,
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
