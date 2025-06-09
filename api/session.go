package api

import (
	"JustSync/service"
	"fmt"
	"log"
	"net/http"
)

func setup(w http.ResponseWriter, r *http.Request) {
	fmt.Println("Setup request recieved")

	err := service.HandleCreateSnapshot("")

	if err != nil {
		panic(err)
	}
}

func HandleRequests() {
	http.HandleFunc("/setup", setup)
	log.Fatal(http.ListenAndServe(":10000", nil))
}
