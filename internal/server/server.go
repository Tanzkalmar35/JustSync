package server

import (
	"JustSync/internal/config"
	api "JustSync/internal/transport/http"
	"JustSync/pkg"
	"net/http"
)

type Server struct {
	config *config.ServerConfig
}

func New(cfg *config.ServerConfig) Server {
	return Server{
		config: cfg,
	}
}

func (s *Server) Run() {
	http.HandleFunc("/connect", api.HandleConnectClient)
	http.HandleFunc("/admin/generateOtp", api.HandleGenerateOtp)

	pkg.LogInfo("Server running at port %s", s.config.Application.Port)

	if err := http.ListenAndServe(s.config.Application.Port, nil); err != nil {
		pkg.LogError(err.Error())
	}
}
