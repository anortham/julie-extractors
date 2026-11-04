package tree_sitter_tree_sitter_vb_dotnet_test

import (
	"testing"

	tree_sitter "github.com/tree-sitter/go-tree-sitter"
	tree_sitter_tree_sitter_vb_dotnet "github.com/tree-sitter/tree-sitter-tree_sitter_vb_dotnet/bindings/go"
)

func TestCanLoadGrammar(t *testing.T) {
	language := tree_sitter.NewLanguage(tree_sitter_tree_sitter_vb_dotnet.Language())
	if language == nil {
		t.Errorf("Error loading TreeSitterVbDotnet grammar")
	}
}
