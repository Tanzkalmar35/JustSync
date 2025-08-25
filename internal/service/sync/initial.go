package sync

import (
	snapshot "JustSync/api"
	"JustSync/pkg"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
)

func (s *SyncService) hydrateFromDisk(service SyncService) error {
	pkg.LogInfo("Initializing service state from disk...")

	return filepath.WalkDir(s.config.Session.PathToCloneFrom, func(path string, d fs.DirEntry, err error) error {
		content, err := os.ReadFile(path)
		if err != nil {
			pkg.LogError("Failed to read file %s: %v", path, err)
			return err
		}

		relativePath, _ := filepath.Rel(s.config.Session.PathToCloneFrom, path)
		doc := CreateDocFromContent(relativePath, content)
		s.documents[relativePath] = doc

		return nil
	})
}

func PrepareReceiveProjectSync() error {
	cfg := utils.GetClientConfig()
	path := filepath.Join(cfg.Session.Path, cfg.Session.Name)

	// Check if destination path already exists
	_, err := os.Stat(path)
	if err == nil {
		err := fmt.Errorf("Folder with name '%s' already exists at '%s'", cfg.Session.Name, cfg.Session.Path)
		pkg.LogError(err.Error())
		return err
	}

	// If the error is anything other than "not exist", it's an unexpected problem (e.g., permissions).
	if !errors.Is(err, fs.ErrNotExist) {
		pkg.LogError("Something went wrong validating project path '%s': %s", path, err.Error())
		return err
	}

	// The directory does not exist, so create it.
	if err := os.MkdirAll(path, 0755); err != nil {
		pkg.LogError("Could not create directory '%s': %s", path, err.Error())
		return err
	}

	return nil
}

// ProcessNewFileSync builds up a file at a given path and fills it with the desired content
func ProcessNewFileSync(syncService SyncService, msg snapshot.WebsocketMessage_InitialFile) error {
	// Build the path for the new file
	cfg := utils.GetClientConfig()
	path := filepath.Join(cfg.Session.Path, cfg.Session.Name, string(msg.InitialFile.Path))
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0755); err != nil {
		pkg.LogError("Unable to create directory structure '%s' due to: %s", dir, err.Error())
		return err
	}

	// Create the actual file
	file, err := os.Create(path)
	if err != nil {
		pkg.LogError("Could not create file %s due to error: %s", path, err.Error())
		return err
	}
	defer file.Close()

	// Fill the file with the actual content
	totalWrittenBytes := 0
	for _, chunk := range msg.InitialFile.File.Chunks {
		b, err := file.WriteAt(chunk.Content, chunk.Offset)
		if err != nil {
			pkg.LogError("Could not write content to file at '%s' due to: %s", msg.InitialFile.Path, err.Error())
			return err
		}
		totalWrittenBytes += b
		pkg.LogDebug("Wrote chunk of size %s to file %s", b, msg.InitialFile.Path)
	}

	// Check content checksum
	pkg.LogDebug("Wrote %b bytes to %s", totalWrittenBytes, msg.InitialFile.Path)
	return nil
}
