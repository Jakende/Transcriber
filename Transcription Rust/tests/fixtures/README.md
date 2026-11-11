`jfk-speech.wav` contains a four-second excerpt of John F. Kennedy's inaugural
address (1961), from whisper.cpp's public `samples/jfk.wav` test recording.
This is a US federal government recording in the public domain. It is used to
verify actual speech detection, which silence-only unit tests cannot establish.

`whisper-mel-reference.json` samples OpenAI Whisper's MIT-licensed mel filter bank.

The `jfk.*` compressed fixtures contain the same public-domain excerpt, encoded
with ffmpeg as development reference data. The application does not invoke ffmpeg.
`picture.mp4` is a synthetic four-second blue frame sequence for muxing tests.
