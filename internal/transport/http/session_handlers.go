package http

import (
	"JustSync/internal/transport/websocket"
	"JustSync/pkg"
	"net/http"
)

func HandleConnectClient(w http.ResponseWriter, r *http.Request) {
	pkg.LogInfo("Client connection request received")

	hub := websocket.GetHub()
	websocket.ServeWs(hub, w, r)

	pkg.LogInfo("New client connected")
}
