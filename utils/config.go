package utils

import (
	"fmt"
	"strings"
)

type RunMode string

type ExternalConfig struct {
	HostUrl    string `yml:"host"`
	ClientName string `yml:"client"`
}

// Set implements flag.Value.
func (e *ExternalConfig) Set(string) error {
	panic("unimplemented")
}

// String implements flag.Value.
func (e *ExternalConfig) String() string {
	return string(e.HostUrl)
}

const (
	ServerMode RunMode = "server"
	ClientMode RunMode = "client"
	AdminMode  RunMode = "admin"
)

var (
	mode RunMode
)

func (m *RunMode) String() string {
	return string(*m)
}

func (m *RunMode) Set(value string) error {
	switch strings.ToLower(value) {
	case "server", "s":
		*m = ServerMode
	case "client", "c":
		*m = ClientMode
	case "admin", "a":
		*m = AdminMode
	default:
		return fmt.Errorf("Invalid mode: %s (valid options: server, client, admin)", value)
	}
	return nil
}

func GetMode() *RunMode {
	return &mode
}

func SetMode(m RunMode) {
	mode = m
}
