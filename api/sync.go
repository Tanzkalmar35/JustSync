package api

import (
	"JustSync/entities"
	"encoding/json"
	"log/slog"
	"net/http"
)

func requestSync(w http.ResponseWriter, r *http.Request) {
	slog.Info("Sync requested")

	var body entities.PathRequest
	if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		slog.Error("Invalid json body data provided")
		return
	}

	// TODO: Get file on path
	// TODO: Hash that file content
	// TODO: compare to state
	// TODO: If change detected, start a sync request to all clients
}
