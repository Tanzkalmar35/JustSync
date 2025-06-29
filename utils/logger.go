package utils

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

func log(level LogLevel, format string, args ...interface{}) {
	if level < logLevel {
		return
	}

	lock.Lock()
	defer lock.Unlock()

	timestamp := time.Now().Format("2006-01-02 15:04:05")
	levelName := levelNames[level]
	message := fmt.Sprintf(format, args...)

	logEntry := fmt.Sprintf(
		"[%s] [%s] [%s] %s\n",
		timestamp,
		GetMode().String(),
		levelName,
		message,
	)

	output.Write([]byte(logEntry))
}

// Convenience methods
func LogDebug(format string, args ...interface{}) {
	log(LevelDebug, format, args...)
}

func LogInfo(format string, args ...interface{}) {
	log(LevelInfo, format, args...)
}

func LogWarn(format string, args ...interface{}) {
	log(LevelWarn, format, args...)
}

func LogError(format string, args ...interface{}) {
	log(LevelError, format, args...)
}
