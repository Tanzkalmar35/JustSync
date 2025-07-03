package api

import (
	"JustSync/utils"
	"fmt"
	"net/http"
)

func HandleGenerateOtp(w http.ResponseWriter, r *http.Request) {
	utils.LogInfo("New one time password requested by admin")
	token := r.URL.Query().Get("t")
	if token == "SECRETKEY" {
		otp := utils.GetTokenManager().GenerateOtp()
		utils.LogInfo("One time password request accepted. Generated %s", otp)
		fmt.Fprintf(w, otp)
		return
	}
}
