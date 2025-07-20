package service

import (
	"JustSync/snapshot"
	"JustSync/utils"
	"errors"
	"io"
	"io/fs"
	"os"
	"path/filepath"
)

func PrepareInitiateProjectSync() ([]snapshot.WebsocketMessage, error) {
	projectRoot := utils.GetHostConfig().Application.Path
	var messages []snapshot.WebsocketMessage
	// Append start sync msg
	startSyncMsg := snapshot.WebsocketMessage_StartSync{}
	messages = append(messages, snapshot.WebsocketMessage{Payload: &startSyncMsg})

	// Append sync msg's for each file
	if err := filepath.WalkDir(projectRoot, func(absolutePath string, d fs.DirEntry, err error) error {
		if err != nil {
			utils.LogError("Error traversing full project for initial sync: %s", err.Error())
			return err
		}

		// Skip directories
		if d.IsDir() {
			return nil
		}

		file, err := os.Open(absolutePath)
		if err != nil {
			utils.LogError("Error reading file at %s: %s", absolutePath, err.Error())
			return err
		}
		defer file.Close()

		relativePath, err := filepath.Rel(projectRoot, absolutePath)
		if err != nil {
			utils.LogError("Could not shrink the absolute path to be relative due to: %s", err.Error())
			return err
		}

		fileChunks, err := utils.ChunkFileContentDefined(file)
		if err != nil {
			utils.LogError("Could not chunk content of file %s due to error: %s", absolutePath, err.Error())
			return err
		}

		fileContent, err := io.ReadAll(file)
		if err != nil {
			utils.LogError("Could not read content of file %s due to error: %s", absolutePath, err.Error())
			return err
		}

		fileSync := &snapshot.InitialSyncFile{
			Checksum: utils.GetHasher()(fileContent),
			Chunks:   fileChunks,
		}
		syncMsg := snapshot.WebsocketMessage{
			Payload: &snapshot.WebsocketMessage_InitialFile{
				InitialFile: &snapshot.InitialSyncFileWithPath{
					Path: []byte(relativePath),
					File: fileSync,
				},
			},
		}
		messages = append(messages, syncMsg)

		return nil
	}); err != nil {
		utils.LogError("Error traversing full project for initial sync: %s", err.Error())
		return messages, err
	}

	// Append end sync msg
	endSyncMsg := snapshot.WebsocketMessage{
		Payload: &snapshot.WebsocketMessage_EndSync{},
	}
	messages = append(messages, endSyncMsg)

	return messages, nil
}

func PrepareReceiveProjectSync() error {
	cfg := utils.GetClientConfig()
	path := cfg.Session.Path + cfg.Session.Name

	_, err := os.Stat(cfg.Session.Path)
	if err == nil {
		utils.LogError("Folder with name %s already existing at %s", cfg.Session.Name, cfg.Session.Path)
		return err
	}
	if !errors.Is(err, fs.ErrNotExist) {
		utils.LogError("Something went wrong validating project path: %s", err.Error())
		return err
	}

	if err := os.Mkdir(path, 0755); err != nil {
		utils.LogError("Could not create directory %s at %s", cfg.Session.Name, cfg.Session.Path)
		return err
	}

	return nil
}

// ApplyFileDelta builds up a file at a given path and fills it with the desired content
func ApplyFileDelta(msg snapshot.WebsocketMessage_FileDelta) error {
	return nil
}
