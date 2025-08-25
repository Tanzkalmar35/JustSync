package config

import (
	"JustSync/pkg"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"

	"gopkg.in/yaml.v3"
)

type RunMode string

type PeerConfig struct {
	Session struct {
		Port            string   `yaml:"port"`
		Name            string   `yaml:"name"`
		PathToCloneTo   string   `yaml:"path"`
		PathToCloneFrom string   `yaml:"path"`
		IgnoredFiles    []string `yaml:"ignoredFiles"`
		Client          struct {
			Name  string `yaml:"name"`
			Token string `yaml:"token"`
		}
	}
}

type ServerConfig struct {
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

	hostConfig      ServerConfig
	hostSingleton   sync.Once
	clientConfig    PeerConfig
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

func InitHostConfig(cfgName string) ServerConfig {
	hostSingleton.Do(func() {
		hostConfig = GetExternalHostConfig(cfgName)
	})

	return hostConfig
}

func GetHostConfig() ServerConfig {
	return hostConfig
}

func InitClientConfig(cfgName string) PeerConfig {
	clientSingleton.Do(func() {
		clientConfig = GetExternalClientConfig(cfgName)
	})

	return clientConfig
}

func GetClientConfig() PeerConfig {
	return clientConfig
}

func GetExternalClientConfig(name string) PeerConfig {
	var config PeerConfig
	path := filepath.Join(GetOsSpecificConfigPath(), name+".yml")
	configContent, err := os.ReadFile(path)
	if err != nil {
		pkg.LogError("Config '%s' not found at os' specific config path '%s'", name, path)
		return config
	}

	if err = yaml.Unmarshal(configContent, &config); err != nil {
		pkg.LogError("Error in config '%s' found. Could not parse config.", name)
		return config
	}

	return config
}

func GetExternalHostConfig(name string) ServerConfig {
	var config ServerConfig
	path := filepath.Join(GetOsSpecificConfigPath(), name+".yml")
	configContent, err := os.ReadFile(path)
	if err != nil {
		pkg.LogError("Config '%s' not found at os' specific config path '%s'", name, path)
		return config
	}

	if err = yaml.Unmarshal(configContent, &config); err != nil {
		pkg.LogError("Error in config '%s' found. Could not parse config.", name)
		return config
	}

	return config
}

func GetOsSpecificConfigPath() string {
	switch runtime.GOOS {
	case "windows": // Well... windows
		return filepath.Join(os.Getenv("APPDATA"), "JustSync")
	case "darwin": // Macos
		return filepath.Join(os.Getenv("HOME"), "Library", "Application Support", "JustSync")
	default: // Linux, BSD, ...
		if xdg := os.Getenv("XDG_CONFIG_HOME"); xdg != "" {
			return filepath.Join(xdg, "JustSync")
		}
		return filepath.Join(os.Getenv("HOME"), ".config", "JustSync")
	}
}
