# Third-party notices

Poiesis voice mode (plans/VOICE_PLAN.md) uses the work below. Each item keeps
its own license. Voice files are downloaded on request and stay on the user's
computer.

## Software

- **sherpa-onnx** (Next-gen Kaldi, k2-fsa). Apache-2.0.
  https://github.com/k2-fsa/sherpa-onnx
  Runs speech detection, hearing and speaking inside the app.
- **Openlive** (byte271). Apache-2.0. https://github.com/byte271/Openlive
  Poiesis took ideas from it and wrote its own code: a playback queue with a
  reply id per chunk, a short fade when a reply is cut off, and per-chunk
  "played" reports (`src/lib/voice/playbackQueue.ts`). Further ideas are
  planned for turn-taking (`TRN-1`, `TRN-2`, `TRN-4`). No Openlive source file
  is included.

## Models (downloaded by the user)

- **Silero VAD** (Silero Team). MIT. https://github.com/snakers4/silero-vad
  File: `silero_vad.onnx`.
- **Parakeet TDT 0.6B v3** (NVIDIA). CC-BY-4.0. Attribution required.
  https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3 . Used as the int8
  conversion published by the sherpa-onnx project.
- **Kokoro 82M** multi-language v1.0 (hexgrad). Apache-2.0. Converted for
  sherpa-onnx by the sherpa-onnx project.
- **Piper voice "Thorsten" (de_DE-thorsten-medium)**. Dataset Thorsten-Voice by
  Thorsten Mueller, CC0. https://github.com/thorstenMueller/Thorsten-Voice
- **Piper voice "Alba" (en_GB-alba-medium)**. Dataset by the University of
  Edinburgh, CC BY 4.0. https://datashare.ed.ac.uk/handle/10283/3270 .
  Attribution required.

Other Piper voices offered in the voice list. Each row gives the voice, who
is heard, the license of its data, and the folder with its model card. Voices
under CC BY need attribution to the dataset authors named in that card.

- en_US-ljspeech-medium (LJ): Public domain. https://huggingface.co/rhasspy/piper-voices/tree/main/en/en_US/ljspeech/medium
- en_US-joe-medium (Joe): CC0. https://huggingface.co/rhasspy/piper-voices/tree/main/en/en_US/joe/medium
- en_US-kristin-medium (Kristin): Public domain. https://huggingface.co/rhasspy/piper-voices/tree/main/en/en_US/kristin/medium
- en_US-norman-medium (Norman): Public domain. https://huggingface.co/rhasspy/piper-voices/tree/main/en/en_US/norman/medium
- en_GB-cori-medium (Cori): Public domain. https://huggingface.co/rhasspy/piper-voices/tree/main/en/en_GB/cori/medium
- de_DE-kerstin-low (Kerstin): CC0. https://huggingface.co/rhasspy/piper-voices/tree/main/de/de_DE/kerstin/low
- fr_FR-siwis-medium (Siwis): CC BY 4.0. https://huggingface.co/rhasspy/piper-voices/tree/main/fr/fr_FR/siwis/medium
- fr_FR-gilles-low (Gilles): CC0. https://huggingface.co/rhasspy/piper-voices/tree/main/fr/fr_FR/gilles/low
- es_ES-davefx-medium (Davefx): CC0. https://huggingface.co/rhasspy/piper-voices/tree/main/es/es_ES/davefx/medium
- es_ES-carlfm-x_low (Carlfm): Public domain. https://huggingface.co/rhasspy/piper-voices/tree/main/es/es_ES/carlfm/x_low
- es_MX-ald-medium (Ald): Unlicense. https://huggingface.co/rhasspy/piper-voices/tree/main/es/es_MX/ald/medium
- es_MX-claude-high (Claude): Apache 2.0. https://huggingface.co/rhasspy/piper-voices/tree/main/es/es_MX/claude/high
- it_IT-paola-medium (Paola): CC0. https://huggingface.co/rhasspy/piper-voices/tree/main/it/it_IT/paola/medium
- nl_NL-pim-medium (Pim): CC0. https://huggingface.co/rhasspy/piper-voices/tree/main/nl/nl_NL/pim/medium
- nl_NL-ronnie-medium (Ronnie): CC0. https://huggingface.co/rhasspy/piper-voices/tree/main/nl/nl_NL/ronnie/medium
- nl_BE-nathalie-medium (Nathalie): CC0. https://huggingface.co/rhasspy/piper-voices/tree/main/nl/nl_BE/nathalie/medium
- pt_BR-faber-medium (Faber): CC0. https://huggingface.co/rhasspy/piper-voices/tree/main/pt/pt_BR/faber/medium
- pt_BR-cadu-medium (Cadu): CC0. https://huggingface.co/rhasspy/piper-voices/tree/main/pt/pt_BR/cadu/medium
- pt_BR-jeff-medium (Jeff): CC0. https://huggingface.co/rhasspy/piper-voices/tree/main/pt/pt_BR/jeff/medium
- pl_PL-gosia-medium (Gosia): CC0. https://huggingface.co/rhasspy/piper-voices/tree/main/pl/pl_PL/gosia/medium
- pl_PL-darkman-medium (Darkman): CC0. https://huggingface.co/rhasspy/piper-voices/tree/main/pl/pl_PL/darkman/medium
- ru_RU-denis-medium (Denis): CC0. https://huggingface.co/rhasspy/piper-voices/tree/main/ru/ru_RU/denis/medium
- ru_RU-dmitri-medium (Dmitri): CC0. https://huggingface.co/rhasspy/piper-voices/tree/main/ru/ru_RU/dmitri/medium
- sv_SE-nst-medium (Nst): CC0. https://huggingface.co/rhasspy/piper-voices/tree/main/sv/sv_SE/nst/medium
- da_DK-talesyntese-medium (Tale): CC0. https://huggingface.co/rhasspy/piper-voices/tree/main/da/da_DK/talesyntese/medium
- no_NO-talesyntese-medium (Tale): CC0. https://huggingface.co/rhasspy/piper-voices/tree/main/no/no_NO/talesyntese/medium
- fi_FI-harri-medium (Harri): CC0. https://huggingface.co/rhasspy/piper-voices/tree/main/fi/fi_FI/harri/medium
- cs_CZ-jirka-medium (Jirka): CC0. https://huggingface.co/rhasspy/piper-voices/tree/main/cs/cs_CZ/jirka/medium
- el_GR-rapunzelina-low (Rapunzelina): CC0. https://huggingface.co/rhasspy/piper-voices/tree/main/el/el_GR/rapunzelina/low
- hu_HU-anna-medium (Anna): CC0. https://huggingface.co/rhasspy/piper-voices/tree/main/hu/hu_HU/anna/medium
- ro_RO-mihai-medium (Mihai): CC0. https://huggingface.co/rhasspy/piper-voices/tree/main/ro/ro_RO/mihai/medium
- vi_VN-vais1000-medium (Vais): CC BY 4.0. https://huggingface.co/rhasspy/piper-voices/tree/main/vi/vi_VN/vais1000/medium

Piper voices are fine-tuned from a base voice; the license of the base data is
linked in each voice's model card (`MODEL_CARD` in its folder).
