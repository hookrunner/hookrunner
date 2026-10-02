package main

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"
)

// Verify at upload time and before publication, so canceled/force-pushed builds
// cannot win a race against a newer commit or a PR close/reopen event.
func (s *service) verifyPR(ctx context.Context, pr int, sha string, closed bool) error {
	ctx, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()
	request, err := http.NewRequestWithContext(ctx, "GET", fmt.Sprintf("https://api.github.com/repos/%s/pulls/%d", s.config.repository, pr), nil)
	if err != nil {
		return err
	}
	request.Header.Set("Accept", "application/vnd.github+json")
	request.Header.Set("User-Agent", "hookrunner-previewd")
	if s.config.githubToken != "" {
		request.Header.Set("Authorization", "Bearer "+s.config.githubToken)
	}
	response, err := http.DefaultClient.Do(request)
	if err != nil {
		return fmt.Errorf("check PR: %w", err)
	}
	defer response.Body.Close()
	if response.StatusCode != 200 {
		return fmt.Errorf("GitHub PR lookup returned %d; check PREVIEW_GITHUB_TOKEN and repository access", response.StatusCode)
	}
	var pull struct {
		State string `json:"state"`
		Head  struct {
			SHA  string `json:"sha"`
			Repo struct {
				FullName string `json:"full_name"`
			} `json:"repo"`
		} `json:"head"`
	}
	if err = json.NewDecoder(io.LimitReader(response.Body, 1<<20)).Decode(&pull); err != nil {
		return err
	}
	if closed {
		if pull.State != "closed" {
			return fmt.Errorf("PR #%d is open; refusing stale cleanup", pr)
		}
		return nil
	}
	if pull.State != "open" || pull.Head.SHA != sha {
		return fmt.Errorf("PR #%d is closed or %s is no longer its head", pr, sha)
	}
	if !strings.EqualFold(pull.Head.Repo.FullName, s.config.repository) {
		return fmt.Errorf("fork previews are disabled")
	}
	return nil
}
