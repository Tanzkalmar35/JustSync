package utils

import (
	"crypto/rand"
	"encoding/base64"
	"sync"
	"time"
)

const (
	OtpLength       = 16
	OtpExpiration   = 10 * time.Minute
	TokenLength     = 32
	TokenExpiration = 24 * time.Hour
)

var (
	instance *TokenManager
	once     sync.Once
)

type TokenManager struct {
	otps       map[string]time.Time // OTP -> expiration
	tokens     map[string]time.Time // token -> expiration
	otpsMutex  sync.RWMutex
	tokenMutex sync.RWMutex
}

func GetTokenManager() *TokenManager {
	once.Do(func() {
		instance = &TokenManager{
			otps:   make(map[string]time.Time),
			tokens: make(map[string]time.Time),
		}
		go instance.CleanUpReguarly()
	})
	return instance
}

func (m *TokenManager) GenerateOtp() string {
	b := make([]byte, OtpLength)
	rand.Read(b)
	otp := base64.URLEncoding.EncodeToString(b)

	m.otpsMutex.Lock()
	m.otps[otp] = time.Now().Add(OtpExpiration)
	m.otpsMutex.Unlock()

	return otp
}

func (m *TokenManager) ValidateOtp(otp string) bool {
	m.otpsMutex.Lock()
	defer m.otpsMutex.Unlock()

	expiration, existing := m.otps[otp]

	if !existing {
		return false
	}

	// Delete otp from memory, therefore invalidating it
	delete(m.otps, otp)

	return time.Now().Before(expiration)
}

func (m *TokenManager) GenerateToken() string {
	b := make([]byte, TokenLength)
	rand.Read(b)
	token := base64.URLEncoding.EncodeToString(b)

	m.tokenMutex.Lock()
	m.tokens[token] = time.Now().Add(TokenExpiration)
	m.tokenMutex.Unlock()

	return token
}

func (m *TokenManager) ValidateToken(token string) bool {
	m.tokenMutex.Lock()
	defer m.tokenMutex.Unlock()

	expiration, existing := m.tokens[token]

	if !existing {
		return false
	}

	delete(m.tokens, token)

	return time.Now().Before(expiration)
}

func (m *TokenManager) CleanUpReguarly() {
	ticker := time.NewTicker(10 * time.Minute)
	for range ticker.C {
		// Cleanup all expired otps and tokens
		now := time.Now()

		m.otpsMutex.Lock()
		for otp, exp := range m.otps {
			if now.After(exp) {
				delete(m.otps, otp)
			}
		}
		m.otpsMutex.Unlock()

		m.tokenMutex.Lock()
		for token, exp := range m.tokens {
			if now.After(exp) {
				delete(m.tokens, token)
			}
		}
		m.tokenMutex.Unlock()
	}
}
