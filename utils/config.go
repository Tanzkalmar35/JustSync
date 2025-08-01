package utils

import (
	"fmt"
	"strings"
	"sync"
)

type RunMode string

type ExternalClientConfig struct {
	Session struct {
		Port string `yml:"port"`
		Name string `yml:"name"`
		Path string `yml:"path"`
		Host struct {
			Url string `yml:"url"`
		}
		Client struct {
			Name  string `yml:"name"`
			Token string `yml:"token"`
		}
	}
}

type ExternalHostConfig struct {
	Application struct {
		Port string
		Path string
	}
}

const (
	ServerMode RunMode = "server"
	ClientMode RunMode = "client"
	AdminMode  RunMode = "admin"
)

var (
	mode RunMode

	hostConfig      ExternalHostConfig
	hostSingleton   sync.Once
	clientConfig    ExternalClientConfig
	clientSingleton sync.Once
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

func InitHostConfig(cfgName string) ExternalHostConfig {
	hostSingleton.Do(func() {
		hostConfig = GetExternalHostConfig(cfgName)
	})

	return hostConfig
}

func GetHostConfig() ExternalHostConfig {
	return hostConfig
}

func InitClientConfig(cfgName string) ExternalClientConfig {
	clientSingleton.Do(func() {
		clientConfig = GetExternalClientConfig(cfgName)
	})

	return clientConfig
}

func GetClientConfig() ExternalClientConfig {
	return clientConfig
}
