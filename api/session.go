package api

import (
	"JustSync/entities"
	"JustSync/service"
	"encoding/json"
	"log/slog"
	"net/http"
)

// Accepts json data
func Setup(w http.ResponseWriter, r *http.Request) {
	slog.Info("Setup requested")

	var body entities.PathRequest
	err := json.NewDecoder(r.Body).Decode(&body)

	if err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		slog.Error("Invalid json body data given")
		return
	}

	err = service.HandleCreateSnapshot(body.Path)

	if err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		slog.Error("Could not create snapshot, probably an invalid path")
		return
	}

	slog.Info("Setup successful")
	w.WriteHeader(http.StatusOK)
}

func ConnectClient(w http.ResponseWriter, r *http.Request) {
	slog.Info("Client connection request received")

	// PERF: Consider switching to websockets later on for truly real time data
	// TODO: Establish connection using SSE (Server-Sent Events)

	slog.Info("Client connected")
}
