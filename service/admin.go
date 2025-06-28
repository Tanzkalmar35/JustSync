package service

import "JustSync/utils"

func HandleCreateOtp() string {
	return utils.GetTokenManager().GenerateOtp()
}
