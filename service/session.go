package service

import (
	"JustSync/snapshot"
	"JustSync/utils"
	"time"

	"github.com/gorilla/websocket"
	"google.golang.org/protobuf/proto"
)

func HandleCreateSnapshot(path string) error {
	snappath := "snapshot/SNAPSHOT.sync.snap"

	snap, err := utils.ProcessDir(path)

	if err != nil {
		return err
	}

	snapshot.WriteSnapshot(snap, snappath)

	utils.LogInfo("Created new snapshot at: %s", snappath)
	return nil
}

func HandleReceiveAndProcessIncomingMessages(conn *websocket.Conn) {
	for {
		msgType, rawMsg, err := conn.ReadMessage() // <-- _ = msg
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

		var msg snapshot.SyncFileMessage
		if err := proto.Unmarshal(rawMsg, &msg); err != nil {
			utils.LogError("Failed to unmarshal protobuf message received from websocket: %s", err.Error())
			continue
		}

		switch t := msg.Payload.(type) {
		case *snapshot.SyncFileMessage_StartSync:
			utils.LogInfo("Initial project sync started. Copying project to %s", utils.GetClientConfig().Session.Path)
			if err := PrepareReceiveProjectSync(); err != nil {
				utils.LogError("Failed to prepare project sync: %s", err.Error())
			}
		case *snapshot.SyncFileMessage_File:
			utils.LogInfo("Received file: %s", t.File.Path)
			start := time.Now()
			if err := ProcessNewFileSync(*t); err != nil {
				utils.LogError("Could not process file sync of file '%s' due to %s", t.File.Path, err.Error())
			}
			elapsed := time.Since(start)
			utils.LogInfo("Successfully processed %s in %s", t.File.Path, elapsed)
		case *snapshot.SyncFileMessage_EndSync:
			utils.LogInfo("Finished sync!")
		default:
			utils.LogError("Recieved message of unexpected type: %T", t)
		}
	}
}
