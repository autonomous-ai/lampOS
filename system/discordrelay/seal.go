package discordrelay

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"errors"
	"strings"
	"time"
)

// Relay messages can travel sealed, so the transport in between sees neither the
// text nor the Discord ids:
//
//	key   = HMAC-SHA256(key = the device's cloud API key, msg = "discord-relay/v1")
//	seal  = AES-256-GCM, random 12-byte nonce, AAD = "<kind>|<device id>"
//	wire  = {"enc":"v1","sealed": base64(nonce || ciphertext)}
//
// The plaintext is the JSON the message would otherwise carry in clear, plus "ts"
// (unix seconds). The AAD ties a sealed message to its kind and to this device, and
// "ts" limits how long it can be replayed. The cloud decides when to seal; the
// device opens what arrives sealed and seals its answer to a sealed message.

const (
	// SealVersion is the "enc" value of a sealed message.
	SealVersion = "v1"
	sealKeyInfo = "discord-relay/v1"
	// sealMaxSkew bounds how old (or how far ahead) a sealed message may be.
	sealMaxSkew = 15 * time.Minute
)

var (
	// ErrSealNoKey: the device has no cloud API key yet.
	ErrSealNoKey = errors.New("discord relay: no device key to seal with")
	// ErrSealVersion: the message uses a seal version this firmware does not know.
	ErrSealVersion = errors.New("discord relay: unknown seal version")
	// ErrSealOpen: the message was not sealed for this device, or was altered.
	ErrSealOpen = errors.New("discord relay: sealed message does not open")
	// ErrSealStale: the message is too old, or the device clock is far off.
	ErrSealStale = errors.New("discord relay: sealed message is too old or from the future")
)

func newGCM(deviceKey string) (cipher.AEAD, error) {
	deviceKey = strings.TrimSpace(deviceKey)
	if deviceKey == "" {
		return nil, ErrSealNoKey
	}
	m := hmac.New(sha256.New, []byte(deviceKey))
	m.Write([]byte(sealKeyInfo))
	block, err := aes.NewCipher(m.Sum(nil))
	if err != nil {
		return nil, err
	}
	return cipher.NewGCM(block)
}

func sealAAD(kind, deviceID string) []byte { return []byte(kind + "|" + deviceID) }

// Seal encrypts body ("ts" is added) for this device and returns the sealed field.
func Seal(deviceKey, kind, deviceID string, body map[string]any, now time.Time) (string, error) {
	aead, err := newGCM(deviceKey)
	if err != nil {
		return "", err
	}
	plain := make(map[string]any, len(body)+1)
	for k, v := range body {
		plain[k] = v
	}
	plain["ts"] = now.Unix()
	pt, err := json.Marshal(plain)
	if err != nil {
		return "", err
	}
	nonce := make([]byte, aead.NonceSize())
	if _, err := rand.Read(nonce); err != nil {
		return "", err
	}
	return base64.StdEncoding.EncodeToString(aead.Seal(nonce, nonce, pt, sealAAD(kind, deviceID))), nil
}

// Open decrypts a sealed message for this device into dst and checks its age.
func Open(deviceKey, kind, deviceID, enc, sealed string, dst any, now time.Time) error {
	if enc != SealVersion {
		return ErrSealVersion
	}
	aead, err := newGCM(deviceKey)
	if err != nil {
		return err
	}
	raw, err := base64.StdEncoding.DecodeString(sealed)
	ns := aead.NonceSize()
	if err != nil || len(raw) < ns+aead.Overhead() {
		return ErrSealOpen
	}
	pt, err := aead.Open(nil, raw[:ns], raw[ns:], sealAAD(kind, deviceID))
	if err != nil {
		return ErrSealOpen
	}
	var ts struct {
		TS int64 `json:"ts"`
	}
	if json.Unmarshal(pt, &ts) != nil {
		return ErrSealOpen
	}
	if d := now.Sub(time.Unix(ts.TS, 0)); d > sealMaxSkew || d < -sealMaxSkew {
		return ErrSealStale
	}
	if json.Unmarshal(pt, dst) != nil {
		return ErrSealOpen
	}
	return nil
}
