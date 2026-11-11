Upstream: silero-vad-pure 0.1.1 (MIT).

The streaming wrapper now prepends and retains the model context (64 samples at 16 kHz, 32 at 8 kHz), as the upstream Silero JIT wrapper does. The published crate omitted this context and failed to detect speech in both a real German recording and the public JFK sample. State reset also clears the context. The neural weights are unchanged.
