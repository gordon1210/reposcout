// Sample Go file for reposcout fixtures.
package sample

import (
	"fmt"
	"strings"
)

// Greet builds a greeting for the given name.
func Greet(name string) string {
	name = strings.TrimSpace(name)
	if name == "" {
		name = "world"
	}
	return fmt.Sprintf("hello, %s", name)
}

func Count(words []string) map[string]int {
	counts := make(map[string]int)
	for _, w := range words {
		counts[w]++
	}
	return counts
}
