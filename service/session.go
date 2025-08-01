package service

import (
	"JustSync/snapshot"
	"JustSync/utils"
	"time"

	"github.com/gorilla/websocket"
	"google.golang.org/protobuf/proto"
)

func HandleCreateSnapshot(path string) error {
	utils.LogInfo("Creating new snapshot at %s", path)
	snap, err := utils.CreateSnapshotOfDir(path)

	if err != nil {
		return err
	}

	snapshot.WriteSnapshot(snap)

	utils.LogInfo("Created new snapshot at: %s", snapshot.SnapPath)
	return nil
}

// CLIENT: Main event loop
func HandleReceiveAndProcessIncomingMessages(conn *websocket.Conn) {
	for {
		msgType, rawMsg, err := conn.ReadMessage()
		if err != nil {
			utils.LogError("An error occured while receiving message from host: %s", err.Error())
			break
		}
		if msgType == websocket.CloseMessage {
			if closeErr, ok := err.(*websocket.CloseError); ok {
				utils.LogInfo("Connection closed by host. Code: %d, Text: %s", closeErr.Code, closeErr.Text)
			} else {
				utils.LogInfo("Connection closed by host")
			}
			return
		}

		var msg snapshot.WebsocketMessage
		if err := proto.Unmarshal(rawMsg, &msg); err != nil {
			utils.LogError("Failed to unmarshal protobuf message received from websocket: %s", err.Error())
			continue
		}

		switch t := msg.Payload.(type) {
		case *snapshot.WebsocketMessage_StartSync:
			utils.LogInfo("Preparing to sync!")
			if err := PrepareReceiveProjectSync(); err != nil {
				utils.LogError("Something went wrong preparing to receive initial project sync: %s", err.Error())
				return
			}
		case *snapshot.WebsocketMessage_FileDelta:
			utils.LogInfo("Received file: %s", t.FileDelta.Path)
			start := time.Now()
			if err := ApplyFileDelta(*t); err != nil {
				utils.LogError("Could not process file sync of file '%s' due to %s", t.FileDelta.Path, err.Error())
			}
			elapsed := time.Since(start)
			utils.LogInfo("Successfully processed %s in %s", t.FileDelta.Path, elapsed)
		case *snapshot.WebsocketMessage_InitialFile:
			utils.LogInfo("Received file: %s", t.InitialFile.Path)
			start := time.Now()
			if err := ProcessNewFileSync(*t); err != nil {
				utils.LogError("Could not process file sync of file '%s' due to %s", t.InitialFile.Path, err.Error())
			}
			elapsed := time.Since(start)
			utils.LogInfo("Successfully processed %s in %s", t.InitialFile.Path, elapsed)
		case *snapshot.WebsocketMessage_EndSync:
			utils.LogInfo("Finishing sync up!")
			HandleCreateSnapshot(utils.GetClientConfig().Session.Path)
		default:
			utils.LogError("Recieved message of unexpected type: %T", t)
		}
	}
}
