package utils

import (
	"fmt"
	"strings"
	"sync"
)

type RunMode string

type ExternalClientConfig struct {
	Session struct {
		Port            string   `yaml:"port"`
		Name            string   `yaml:"name"`
		PathToCloneTo   string   `yaml:"path"`
		PathToCloneFrom string   `yaml:"path"`
		IgnoredFiles    []string `yaml:"ignoredFiles"`
		Host            struct {
			Url string `yaml:"url"`
		}
		Client struct {
			Name  string `yaml:"name"`
			Token string `yaml:"token"`
		}
	}
}

type ExternalHostConfig struct {
	Application struct {
		Port string `yaml:"port"`
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
