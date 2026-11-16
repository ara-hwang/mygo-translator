package core

// Server numeric limits are shared by persisted-settings sanitization and form
// validation; each caller retains its own clamp-or-reject policy.
const (
	serverPortMin      = 1024
	serverPortMax      = 65535
	serverCtxSizeMin   = 512
	serverCtxSizeMax   = 32768
	serverThreadsMin   = 1
	serverThreadsMax   = 64
	serverGPULayersMin = -1
	serverGPULayersMax = 256
)
