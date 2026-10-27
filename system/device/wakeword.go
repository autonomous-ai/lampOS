package device

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"sync"
	"time"

	"go.autonomous.ai/os/system/domain"
	"go.autonomous.ai/os/system/lib/hal"
	"go.autonomous.ai/os/system/server/config"
)

const wakeWordApplyTimeout = 30 * time.Second

// wakeWordApply serializes HTTP/MQTT saves and keeps failed applies retryable.
// pending is independent of the desired config, which can change even on a
// failed save. Each flag clears only after its corresponding HAL apply succeeds.
type wakeWordApply struct {
	mu          sync.Mutex
	pending     bool
	modePending bool
}

// UpdateWakeWord persists the gate and waits for any required HAL restart.
// A failed save/restart leaves pending set so an unchanged retry can recover.
func (s *Service) UpdateWakeWord(enabled bool) error {
	s.wakeApply.mu.Lock()
	defer s.wakeApply.mu.Unlock()

	if err := s.config.WithLockSave(func(c *config.Config) {
		changed := applyWakeWord(c, enabled)
		s.wakeApply.pending = s.wakeApply.pending || changed
	}); err != nil {
		return fmt.Errorf("save config: %w", err)
	}

	if !s.wakeApply.pending && !s.wakeApply.modePending {
		slog.Info("wakeword config unchanged", "component", "device", "enabled", enabled)
		return nil
	}
	slog.Info("wakeword config updated", "component", "device", "enabled", enabled)
	return s.applyPendingWakeWord()
}

// applyPendingWakeWord requires wakeApply.mu and a successful config save.
func (s *Service) applyPendingWakeWord() error {
	ctx, cancel := context.WithTimeout(context.Background(), wakeWordApplyTimeout)
	defer cancel()
	if s.wakeApply.pending {
		if err := s.restartHALAndWait(ctx, "wake/boot config change"); err != nil {
			return err
		}
		s.wakeApply.pending = false
	} else if s.wakeApply.modePending {
		apply := s.halInputModeApply
		if apply == nil {
			apply = hal.SetVoiceInputMode
		}
		if err := applyInputModeLive(ctx, apply, s.config.GetVoiceInputMode(), s.config.WakeWordEnabled()); err != nil {
			// The mode is already saved and HAL reads it at boot, so restart
			// once rather than leave HAL in the old mode under a config that
			// reports the new one. No further retry: a failed restart is
			// returned and stays pending for an explicit retry.
			slog.Warn("live voice input mode apply failed; restarting HAL to load it",
				"component", "device", "mode", s.config.GetVoiceInputMode(), "error", err)
			restartCtx, restartCancel := context.WithTimeout(context.Background(), wakeWordApplyTimeout)
			defer restartCancel()
			if rerr := s.restartHALAndWait(restartCtx, "voice input mode apply failed"); rerr != nil {
				return fmt.Errorf("apply voice input mode: %w", errors.Join(err, rerr))
			}
		}
	}
	s.wakeApply.modePending = false
	return nil
}

// inputModeLiveRetryFor bounds how long a refused live mode switch is retried.
// HAL answers 503 while it starts (e.g. right after a wake settings restart)
// or while voice teardown runs; both clear within seconds.
var inputModeLiveRetryFor = 12 * time.Second

const inputModeLiveRetryEvery = time.Second

// applyInputModeLive asks the running HAL to switch modes, retrying a refusal
// for up to inputModeLiveRetryFor within ctx. It returns the last error.
func applyInputModeLive(ctx context.Context, apply func(context.Context, string, bool) error, mode string, wakeword bool) error {
	deadline := time.Now().Add(inputModeLiveRetryFor)
	for {
		err := apply(ctx, mode, wakeword)
		if err == nil || ctx.Err() != nil || !time.Now().Add(inputModeLiveRetryEvery).Before(deadline) {
			return err
		}
		select {
		case <-ctx.Done():
			return err
		case <-time.After(inputModeLiveRetryEvery):
		}
	}
}

// applyWakeWord updates the flag and reports whether its effective value changed.
func applyWakeWord(c *config.Config, enabled bool) bool {
	changed := c.WakeWordEnabled() != enabled
	c.WakeWord = &enabled
	return changed
}

// UpdateVoiceInputMode shares the HTTP persistence/apply path and its retry state.
func (s *Service) UpdateVoiceInputMode(mode string) error {
	return s.UpdateConfig(domain.UpdateConfigRequest{VoiceInputMode: &mode})
}

// ToggleVoiceInputMode reads, persists and applies under the same lock as explicit
// HTTP/MQTT updates, so a gesture cannot overwrite a concurrent mode selection.
func (s *Service) ToggleVoiceInputMode() (string, error) {
	s.wakeApply.mu.Lock()
	defer s.wakeApply.mu.Unlock()
	var mode string
	if err := s.config.WithLockSave(func(c *config.Config) {
		mode = config.VoiceInputTapToTalk
		if c.GetVoiceInputMode() == config.VoiceInputTapToTalk {
			mode = config.VoiceInputAutomatic
		}
		c.VoiceInputMode = mode
		s.wakeApply.modePending = true
	}); err != nil {
		return "", fmt.Errorf("save config: %w", err)
	}
	if err := s.applyPendingWakeWord(); err != nil {
		return "", err
	}
	return mode, nil
}
