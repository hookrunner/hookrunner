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
func (s *service) githubJSON(ctx context.Context, path string, value any) error {
	ctx, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()
	request, err := http.NewRequestWithContext(ctx, "GET", fmt.Sprintf("https://api.github.com/repos/%s/%s", s.config.repository, path), nil)
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
		return fmt.Errorf("check GitHub: %w", err)
	}
	defer response.Body.Close()
	if response.StatusCode != 200 {
		return fmt.Errorf("GitHub %s lookup returned %d; check PREVIEW_GITHUB_TOKEN and repository access", path, response.StatusCode)
	}
	return json.NewDecoder(io.LimitReader(response.Body, 1<<20)).Decode(value)
}

func (s *service) verifyMain(ctx context.Context, sha string) error {
	var ref struct {
		Object struct {
			SHA string `json:"sha"`
		} `json:"object"`
	}
	if err := s.githubJSON(ctx, "git/ref/heads/main", &ref); err != nil {
		return err
	}
	if ref.Object.SHA != sha {
		return fmt.Errorf("%s is no longer the main branch head", sha)
	}
	return nil
}

func (s *service) verifyPR(ctx context.Context, pr int, sha string, closed bool) error {
	var pull struct {
		State string `json:"state"`
		Head  struct {
			SHA  string `json:"sha"`
			Repo struct {
				FullName string `json:"full_name"`
			} `json:"repo"`
		} `json:"head"`
	}
	if err := s.githubJSON(ctx, fmt.Sprintf("pulls/%d", pr), &pull); err != nil {
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
