package utils

import (
	"JustSync/snapshot"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"time"

	"github.com/zeebo/blake3"
	"gopkg.in/yaml.v3"
)

const (
	ChunkSize = 4 * 1024 // 4096 bytes (4kb)
)

func ProcessDir(root string) (*snapshot.ProjectSnapshot, error) {
	snap := &snapshot.ProjectSnapshot{
		Version:   "1.0",
		Timestamp: time.Now().UnixNano(),
		Files:     map[string]*snapshot.FileChunks{},
	}

	if info, err := os.Stat(root); err != nil {
		return snap, fmt.Errorf("Invalid path: %w", err)
	} else if !info.IsDir() {
		return snap, errors.New("Path does not point to a directory")
	}

	if err := filepath.WalkDir(root, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return fmt.Errorf("access error at %s: %w", path, err)
		}

		// Skip directories
		if d.IsDir() {
			return nil
		}

		filesnap, e := processFile(path)

		if e != nil {
			// Handle but don't abort on file processing errors
			return fmt.Errorf("processing error: %v\n", err)
		}

		snap.Files[path] = &filesnap

		return nil
	}); err != nil {
		return snap, err
	}

	return snap, nil
}

func processFile(path string) (snapshot.FileChunks, error) {
	snap := snapshot.FileChunks{
		WholeHash:   []byte{},
		ChunkHashes: [][]byte{},
	}

	// PERF: Consider streaming file content instead of loading full content into memory. However for now, as we are mostly working with <1mb files, this is still fine
	filecontent, err := os.ReadFile(path)

	if err != nil {
		return snap, err
	}

	// Hash whole content
	snap.WholeHash = CreateBlake3Hash(filecontent)

	// Split into chunks and hash these
	// PERF: Implement smart chunking based on file size instead of fixed size
	chunkHashes, err := chunkFileContentFixedSize(filecontent)

	if err != nil {
		return snap, err
	}

	snap.ChunkHashes = chunkHashes

	LogDebug(strconv.Itoa(len(snap.ChunkHashes)))

	for i, hashes := range snap.ChunkHashes {
		LogDebug("Hahes %s holds: %s", string(rune(i)), string(hashes))
	}

	return snap, nil
}

func chunkFileContentFixedSize(filecontent []byte) ([][]byte, error) {
	var chunkHashes [][]byte

	for offset := 0; offset < len(filecontent); offset += ChunkSize {
		end := min(offset+ChunkSize, len(filecontent))

		chunk := filecontent[offset:end]
		LogDebug("Processing chunk: %s", string(chunk))
		chunkHashes = append(chunkHashes, CreateBlake3Hash(chunk))
	}

	return chunkHashes, nil
}

func CreateBlake3Hash(data []byte) []byte {
	hasher := blake3.New()
	hasher.Write(data)
	return hasher.Sum(nil)
}

func GetOsSpecificConfigPath() string {
	switch runtime.GOOS {
	case "windows": // Well... windows
		return filepath.Join(os.Getenv("APPDATA"), "JustSync")
	case "darwin": // Macos
		return filepath.Join(os.Getenv("HOME"), "Library", "Application Support", "JustSync")
	default: // Linux, BSD, ...
		if xdg := os.Getenv("XDG_CONFIG_HOME"); xdg != "" {
			return filepath.Join(xdg, "JustSync")
		}
		return filepath.Join(os.Getenv("HOME"), ".config", "JustSync")
	}
}

func CreateConfigFolderAt(path string) {
	if _, err := os.Stat(path); os.IsNotExist(err) {
		if err := os.MkdirAll(path, 0755); err != nil {
			LogError("Config could not be initialized at %s due to %s", path, err.Error())
		} else {
			LogInfo("Created config at %s", path)
		}
	} else {
		LogInfo("Config directory does already exist")
	}
}

func GetExternalClientConfig(name string) ExternalClientConfig {
	var config ExternalClientConfig
	path := filepath.Join(GetOsSpecificConfigPath(), name+".yml")
	configContent, err := os.ReadFile(path)
	if err != nil {
		LogError("Config '%s' not found at os' specific config path '%s'", name, path)
		return config
	}

	if err = yaml.Unmarshal(configContent, &config); err != nil {
		LogError("Error in config '%s' found. Could not parse config.", name)
		return config
	}

	return config
}

func GetExternalHostConfig(name string) ExternalHostConfig {
	var config ExternalHostConfig
	path := filepath.Join(GetOsSpecificConfigPath(), name+".yml")
	configContent, err := os.ReadFile(path)
	if err != nil {
		LogError("Config '%s' not found at os' specific config path '%s'", name, path)
		return config
	}

	if err = yaml.Unmarshal(configContent, &config); err != nil {
		LogError("Error in config '%s' found. Could not parse config.", name)
		return config
	}

	return config
}
