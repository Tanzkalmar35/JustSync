package api

import (
	"JustSync/service"
	"JustSync/utils"
	"encoding/json"
	"log/slog"
	"net/http"
)

// Accepts json data
func Setup(w http.ResponseWriter, r *http.Request) {
	slog.Info("Setup requested")

	var req struct{ path string }
	err := json.NewDecoder(r.Body).Decode(&req)

	if err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		slog.Error("Invalid json body data given")
		return
	}

	err = service.HandleCreateSnapshot(req.path)

	if err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		slog.Error("Could not create snapshot, probably an invalid path")
		return
	}

	slog.Info("Setup successful")
	w.WriteHeader(http.StatusOK)
}

// PERF: Consider switching to websockets later on for truly real time data
func AuthenticateClient(w http.ResponseWriter, r *http.Request) {
	slog.Info("Client connection request received")

	var req struct{ otp string }

	err := json.NewDecoder(r.Body).Decode(&req)
	if err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		slog.Error("Invalid json body data given")
		return
	}

	if !utils.GetTokenManager().ValidateOtp(req.otp) {
		msg := "Given one time password could not be validated. Remember these invalidate after 10mins"
		http.Error(w, msg, http.StatusForbidden)
		slog.Error(msg)
		return
	}

	resp := "token='" + utils.GetTokenManager().GenerateToken() + "'"
	w.Write([]byte(resp))

	slog.Info("Client connected")
	w.WriteHeader(http.StatusAccepted)
}
