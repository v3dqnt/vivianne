/**
 * Viv Instant and the resting notch are switched off in this release, so their
 * settings are hidden. Mirrors `instant::ENABLED` in the Rust backend, which is
 * what actually turns them off; set both to `true` to bring them back.
 */
export const INSTANT_ENABLED = false
