package main

import (
	"JustSync/api"
	"JustSync/utils"
	"bufio"
	"flag"
	"fmt"
	"log/slog"
	"net/http"
	"os"
	"strings"
)

func main() {
	fmt.Println("Hello, World!")

	// Logger initialization
	logger := slog.New(slog.NewTextHandler(os.Stdout, &slog.HandlerOptions{
		Level: slog.LevelInfo, // Set global level
	}))
	slog.SetDefault(logger)

	adminMode := flag.Bool("admin", false, "Run in admin console mode")
	flag.Parse()

	if *adminMode {
		runAdminMode()
	} else {
		handleRequests()
	}
}

func handleRequests() {
	http.HandleFunc("/setup", api.Setup)
	http.HandleFunc("/send-sync", api.RequestSync)
	if err := http.ListenAndServe(":10000", nil); err != nil {
		slog.Error(err.Error())
	}
}

func runAdminMode() {
	slog.Info("Admin console")
	slog.Info("Commands: new-otp, exit")

	tm := utils.NewTokenManager()
	reader := bufio.NewReader(os.Stdin)

	for {
		fmt.Print("> ")
		input, _ := reader.ReadString('\n')
		input = strings.TrimSpace(input)

		switch input {
		case "new-otp":
			otp := tm.GenerateOtp()
			slog.Info("Generated otp: %s\n", otp)
			slog.Info("Generated otp expires in %.0f minutes\n", utils.OtpExpiration.Minutes())
		case "exit":
			os.Exit(0)
		default:
			slog.Error("Unknown command.")
		}
	}
}
