package main

import "C"

//export add
func add(a, b int32) int32 {
	return a + b
}
func main() {}
