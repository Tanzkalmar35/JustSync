package main

import (
	"JustSync/api"
	"fmt"
	"log/slog"
	"net/http"
	"os"
)

func main() {
	fmt.Println("Hello, World!")

	// Logger initialization
	logger := slog.New(slog.NewTextHandler(os.Stdout, &slog.HandlerOptions{
		Level: slog.LevelInfo, // Set global level
	}))
	slog.SetDefault(logger)

	handleRequests()
}

func handleRequests() {
	http.HandleFunc("/setup", api.Setup)
	http.HandleFunc("/send-sync", api.RequestSync)
	if err := http.ListenAndServe(":10000", nil); err != nil {
		slog.Error(err.Error())
	}
}
