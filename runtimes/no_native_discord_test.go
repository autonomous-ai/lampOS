package runtimes_test

import (
	"go/parser"
	"go/token"
	"io/fs"
	"path/filepath"
	"strings"
	"testing"
)

// Discord messages go through the cloud relay, so no runtime or system package opens
// a Discord gateway session of its own.
func TestNoNativeDiscordGatewayLogin(t *testing.T) {
	var offenders []string
	for _, root := range []string{".", filepath.Join("..", "system")} {
		err := filepath.WalkDir(root, func(path string, d fs.DirEntry, err error) error {
			if err != nil {
				return err
			}
			if d.IsDir() || !strings.HasSuffix(path, ".go") || strings.HasSuffix(path, "_test.go") {
				return nil
			}
			f, perr := parser.ParseFile(token.NewFileSet(), path, nil, parser.ImportsOnly)
			if perr != nil {
				return perr
			}
			for _, imp := range f.Imports {
				if strings.Contains(imp.Path.Value, "discordgo") {
					offenders = append(offenders, path)
				}
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	}
	if len(offenders) > 0 {
		t.Fatalf("native Discord gateway client imported by: %v", offenders)
	}
}
