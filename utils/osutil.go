package utils

import (
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
)

func ProcessDir(root string) error {
	if info, err := os.Stat(root); err != nil {
		return fmt.Errorf("Invalid path: %w", err)
	} else if !info.IsDir() {
		return errors.New("Path does not point to a directory")
	}

	return filepath.WalkDir(root, func(path string, d fs.DirEntry, err error) error {
		// Handle directory traversal errors
		if err != nil {
			return fmt.Errorf("access error at %s: %w", path, err)
		}

		// Skip directories
		if d.IsDir() {
			return nil
		}

		// Process file (replace this with your actual logic)
		if err := processFile(path); err != nil {
			// Handle but don't abort on file processing errors
			fmt.Printf("processing error: %v\n", err)
		}

		return nil
	})
}

func processFile(path string) error {
	content, err := os.ReadFile(path)

	if err != nil {
		return err
	}

	fmt.Printf("\n═════ File: %s ═════\n", path)
	fmt.Println(string(content))
	fmt.Println("═══════════════════════════════════════════════")

	return nil
}
