package api

import (
	"JustSync/entities"
	"JustSync/service"
	"encoding/json"
	"log/slog"
	"net/http"
)

// Accepts json data
func setup(w http.ResponseWriter, r *http.Request) {
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
		return
	}

	slog.Info("Setup successful")
}

func HandleRequests() {
	http.HandleFunc("/setup", setup)
	if err := http.ListenAndServe(":10000", nil); err != nil {
		slog.Error(err.Error())
	}
}
