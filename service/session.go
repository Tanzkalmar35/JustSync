package service

import (
	"JustSync/snapshot"
	"JustSync/utils"

	"github.com/gorilla/websocket"
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
		msgType, _, err := conn.ReadMessage() // <-- _ = msg
		if err != nil {
			utils.LogError("An error occured while receiving message from host: %s", err.Error())
			break
		}
		if msgType == websocket.CloseMessage {
			if closeErr, ok := err.(*websocket.CloseError); ok {
				utils.LogInfo("Connection closed by host. Code: %d, Text: %s", closeErr.Code, closeErr.Text)
			} else {
				utils.LogInfo("Connection closed by host: %s", err.Error()) // Fallback for unexpected error type
			}
			return // Exit the loop as connection is closed
		}

		// TODO: Process msg
	}
}
