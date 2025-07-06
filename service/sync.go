package service

import (
	"JustSync/snapshot"
	"JustSync/utils"
	"JustSync/websocket"
	"io/fs"
	"os"
	"path/filepath"
)

func PrepareInitialSync(c *websocket.Client) ([]snapshot.SyncFileMessage, error) {
	path := utils.GetHostConfig().Application.Path
	var messages []snapshot.SyncFileMessage

	// Append start sync msg
	startSyncMsg := snapshot.SyncFileMessage{
		Payload: &snapshot.SyncFileMessage_StartSync{},
	}
	messages = append(messages, startSyncMsg)

	// Append sync msg's for each file
	if err := filepath.WalkDir(path, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			utils.LogError("Error traversing full project for initial sync: %s", err.Error())
			return err
		}

		// Skip directories
		if d.IsDir() {
			return nil
		}

		fileContent, err := os.ReadFile(path)
		if err != nil {
			utils.LogError("Error reading file at %s: %s", path, err.Error())
			return err
		}

		fileSync := &snapshot.FileSync{
			Checksum: utils.CreateBlake3Hash(fileContent),
			Path:     path,
			Content:  fileContent,
		}
		syncMsg := snapshot.SyncFileMessage{
			Payload: &snapshot.SyncFileMessage_File{File: fileSync},
		}
		messages = append(messages, syncMsg)

		return nil
	}); err != nil {
		utils.LogError("Error traversing full project for initial sync: %s", err.Error())
		return messages, err
	}

	// Append end sync msg
	endSyncMsg := snapshot.SyncFileMessage{
		Payload: &snapshot.SyncFileMessage_EndSync{},
	}
	messages = append(messages, endSyncMsg)

	return messages, nil
}
