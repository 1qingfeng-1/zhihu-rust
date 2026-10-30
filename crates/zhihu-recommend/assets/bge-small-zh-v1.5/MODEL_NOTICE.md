# Bundled embedding model

`model_quantized.onnx`, `tokenizer.json` and `config.json` in this directory are
the ONNX export of **BAAI/bge-small-zh-v1.5**, converted by
**Xenova/bge-small-zh-v1.5** on Hugging Face.

- Upstream model: https://huggingface.co/BAAI/bge-small-zh-v1.5
- ONNX export: https://huggingface.co/Xenova/bge-small-zh-v1.5
- License: MIT (declared in the upstream model card)
- Model card license: `mit`
- Downloaded: 2026-10-30, `onnx/model_quantized.onnx` (24,010,842 bytes),
  `tokenizer.json` (439,125 bytes), `config.json` (716 bytes).

The model runs entirely on-device. The app sends nothing to the network for
embedding, and only stores generated vectors and content metadata locally.

## MIT license text

Copyright (c) 2023 Beijing Academy of Artificial Intelligence (BAAI)

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
