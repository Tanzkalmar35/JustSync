package main

import (
	"JustSync/api"
	"fmt"
	"log/slog"
	"os"
)

func main() {
	fmt.Println("Hello, World!")

	// Initialize logger with DEBUG level at startup
	logger := slog.New(slog.NewTextHandler(os.Stdout, &slog.HandlerOptions{
		Level: slog.LevelInfo, // ← Set global level here
	}))
	slog.SetDefault(logger) // ← Make it the global default

	api.HandleRequests()
}
