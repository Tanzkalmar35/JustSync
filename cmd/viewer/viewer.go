package main

import (
	"JustSync/snapshot"
	"encoding/json"
	"fmt"
	"os"

	"google.golang.org/protobuf/proto"
)

func main() {
	if len(os.Args) < 2 {
		fmt.Println("Usage: view_snapshot <file.syncsnap>")
		os.Exit(1)
	}

	data, err := os.ReadFile(os.Args[1])
	if err != nil {
		fmt.Printf("Error reading file: %v\n", err)
		os.Exit(1)
	}

	snap := &snapshot.ProjectSnapshot{}
	if err := proto.Unmarshal(data, snap); err != nil {
		fmt.Printf("Error decoding protobuf: %v\n", err)
		os.Exit(1)
	}

	// Convert to indented JSON
	jsonData, err := json.MarshalIndent(snap, "", "  ")
	if err != nil {
		fmt.Printf("Error marshaling JSON: %v\n", err)
		os.Exit(1)
	}

	fmt.Println(string(jsonData))
}
