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
	if bytes.Equal(hash, snap.Files[body.path].Checksum) {
		utils.LogInfo("Sync request rejected, no change in file detected.")
		return
	}

	// Chunk new file content
	newChunks, err := utils.ChunkFileContentDefined(file)
	if err != nil {
		utils.LogError("An error while chunking file '%s': %s", body.path, err.Error())
		return
	}

	oldChunkMap := make(map[string]*snapshot.InitialSyncChunk) // hash -> Chunk
	newChunkMap := make(map[string]*snapshot.InitialSyncChunk) // hash -> Chunk
	for _, chunk := range snap.Files[body.path].Chunks {
		oldChunkMap[string(chunk.Checksum)] = chunk
	}
	for _, chunk := range newChunks {
		newChunkMap[string(chunk.Checksum)] = chunk
	}

	msg := snapshot.FileDelta{
		Path:               body.path,
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

	// websocket.GetHub().Broadcast <- endMsg
	w.WriteHeader(http.StatusOK)
}

func HeartBeat(w http.ResponseWriter, r *http.Request) {
	utils.LogInfo("Heartbeat received")

	w.WriteHeader(http.StatusOK)
	fmt.Fprintln(w, "Heartbeat successful")
}
