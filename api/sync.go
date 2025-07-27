package api

import (
	"JustSync/snapshot"
	"JustSync/utils"
	socket "JustSync/websocket"
	"bytes"
	"encoding/json"
	"fmt"
	"io"
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

	// Getting file content
	file, err := os.Open(body.Path)
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
	snap := snapshot.GetSnapshot()

	// Checking if changes were made
	if bytes.Equal(hash, snap.Files[body.Path].Checksum) {
		utils.LogInfo("Sync request rejected, no change in file detected.")
		return
	}

	// Chunk new file content
	newChunks, err := utils.ChunkFileContentDefined(file)
	if err != nil {
		utils.LogError("An error while chunking file '%s': %s", body.Path, err.Error())
		return
	}

	// Prepare new snapshot object
	newSnapshot := snapshot.GetSnapshot()
	newSnapshot.Files[body.Path].Checksum = hash

	// Prepare file delta calculation
	oldChunkMap := make(map[string]*snapshot.InitialSyncChunk) // hash -> Chunk
	newChunkMap := make(map[string]*snapshot.InitialSyncChunk) // hash -> Chunk
	for _, chunk := range snap.Files[body.Path].Chunks {
		oldChunkMap[string(chunk.Checksum)] = chunk
	}
	for _, chunk := range newChunks {
		newChunkMap[string(chunk.Checksum)] = chunk
		newSnapshot.Files[body.Path].Chunks = append(newSnapshot.Files[body.Path].Chunks, chunk)
	}

	snapshot.WriteSnapshot(newSnapshot)

	msg := snapshot.FileDelta{
		Path:               body.Path,
		Checksum:           hash,
		AddedChunks:        []*snapshot.AddedChunk{},
		MovedChunks:        []*snapshot.MovedChunk{},
		RemovedChunkHashes: [][]byte{},
	}

	for _, newChunk := range newChunkMap {
		if oldChunk, exists := oldChunkMap[string(newChunk.Checksum)]; !exists {
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

	for hash := range oldChunkMap {
		if _, exists := newChunkMap[hash]; !exists {
			msg.RemovedChunkHashes = append(msg.RemovedChunkHashes, []byte(hash))
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
	socket.GetClient().Conn.WriteMessage(websocket.TextMessage, msgBytes)

	w.WriteHeader(http.StatusOK)

	utils.LogInfo("Sync accepted and sent to host")
}

func HeartBeat(w http.ResponseWriter, r *http.Request) {
	utils.LogInfo("Heartbeat received")

	w.WriteHeader(http.StatusOK)
	fmt.Fprintln(w, "Heartbeat successful")
}
