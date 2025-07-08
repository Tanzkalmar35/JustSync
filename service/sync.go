package service

import (
	"JustSync/snapshot"
	"JustSync/utils"
	"bytes"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
)

func PrepareInitiateProjectSync() ([]snapshot.SyncFileMessage, error) {
	projectRoot := utils.GetHostConfig().Application.Path
	var messages []snapshot.SyncFileMessage

	// Append start sync msg
	startSyncMsg := snapshot.SyncFileMessage{
		Payload: &snapshot.SyncFileMessage_StartSync{},
	}
	messages = append(messages, startSyncMsg)

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

		fileContent, err := os.ReadFile(absolutePath)
		if err != nil {
			utils.LogError("Error reading file at %s: %s", absolutePath, err.Error())
			return err
		}

		relativePath, err := filepath.Rel(projectRoot, absolutePath)
		if err != nil {
			utils.LogError("Could not shrink the absolute path to be relative due to: %s", err.Error())
			return err
		}

		fileSync := &snapshot.FileSync{
			Checksum: utils.CreateBlake3Hash(fileContent),
			Path:     relativePath,
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

// ProcessNewFileSync builds up a file at a given path and fills it with the desired content
func ProcessNewFileSync(msg snapshot.SyncFileMessage_File) error {
	// Check content checksum
	contentHash := utils.CreateBlake3Hash(msg.File.Content)
	if !bytes.Equal(contentHash, msg.File.Checksum) {
		errMsg := "Error during file data transmission, checksum mismatch!"
		utils.LogError("%s", errMsg)
		return fmt.Errorf("%s", errMsg)
	}

	// Build the path for the new file
	cfg := utils.GetClientConfig()
	path := filepath.Join(cfg.Session.Path, cfg.Session.Name, msg.File.Path)
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0755); err != nil {
		utils.LogError("Unable to create directory structure '%s' due to: %s", dir, err.Error())
		return err
	}

	// Create the actual file
	file, err := os.Create(path)
	if err != nil {
		utils.LogError("Could not create file %s due to error: %s", path, err.Error())
		return err
	}

	// Fill the file with the actual content
	b, err := file.Write(msg.File.Content)
	if err != nil {
		utils.LogError("Could not write content to file at '%s' due to: %s", msg.File.Path, err.Error())
		return err
	}

	utils.LogInfo("Wrote %b bytes to %s", b, msg.File.Path)
	return nil
}
