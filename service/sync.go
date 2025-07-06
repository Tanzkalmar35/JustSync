package service

import (
	"JustSync/snapshot"
	"JustSync/utils"
	"JustSync/websocket"
	"fmt"
	"io/fs"
	"path/filepath"
)

func PrepareInitialSync(c *websocket.Client) ([]snapshot.SyncFileMessage, error) {
	path := utils.GetHostConfig().Application.Path
	var messages []snapshot.SyncFileMessage

	if err := filepath.WalkDir(path, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			utils.LogError("Error traversing full project for initial sync: %s", err.Error())
			return fmt.Errorf("access error at %s: %w", path, err)
		}

		// Skip directories
		if d.IsDir() {
			return nil
		}

		// TODO: Prepare and append file msg

		return nil
	}); err != nil {
		utils.LogError("Error traversing full project for initial sync: %s", err.Error())
		return messages, err
	}

	return messages, nil
}
