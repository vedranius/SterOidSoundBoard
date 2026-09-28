# Croatian read speech from Praat's built-in eSpeak synthesizer (VALIDATION.md).
# Usage: praat --run speech_hr.praat   → speech_hr.wav in the current directory
synth = Create SpeechSynthesizer: "Croatian", "Male1"
To Sound: "Dobar dan. Danas je lijep sunčan dan, a mi čitamo standardni tekst za procjenu glasa.", "no"
Resample: 44100, 50
Save as WAV file: "speech_hr.wav"
