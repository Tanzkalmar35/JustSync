package pkg

import (
	"fmt"
	"io"
	"os"
	"sync"
	"time"
)

type LogLevel int

const (
	LevelDebug LogLevel = iota
	LevelInfo
	LevelWarn
	LevelError
)

var (
	levelNames = map[LogLevel]string{
		LevelDebug: "DEBUG",
		LevelInfo:  "INFO",
		LevelWarn:  "WARN",
		LevelError: "ERROR",
	}
	logLevel LogLevel
	lock     sync.Mutex
	output   io.Writer = os.Stdout
)

func SetLevel(level LogLevel) {
	lock.Lock()
	defer lock.Unlock()
	logLevel = level
}

func SetOutput(w io.Writer) {
	lock.Lock()
	defer lock.Unlock()
	output = w
}

func log(level LogLevel, color, format string, args ...any) {
	if level < logLevel {
		return
	}

	lock.Lock()
	defer lock.Unlock()

	timestamp := time.Now().Format("2006-01-02 15:04:05")
	levelName := levelNames[level]
	message := fmt.Sprintf(format, args...)

	logEntry := fmt.Sprintf(
		"[%s] %s [%s] \033[0m %s\n",
		timestamp,
		color,
		levelName,
		message,
	)

	output.Write([]byte(logEntry))
}

// Convenience methods
func LogDebug(format string, args ...any) {
	log(LevelDebug, "\033[32m", format, args...)
}

func LogInfo(format string, args ...any) {
	log(LevelInfo, "\033[34m", format, args...)
}

func LogWarn(format string, args ...any) {
	log(LevelWarn, "\033[33m", format, args...)
}

func LogError(format string, args ...any) {
	log(LevelError, "\033[31m", format, args...)
}
