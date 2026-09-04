# Word frequency lists

`de.txt` and `en.txt` are the first 20 000 entries of the 2018 OpenSubtitles frequency lists
from [hermitdave/FrequencyWords](https://github.com/hermitdave/FrequencyWords), restricted to
words made only of lowercase letters (a-z, ä, ö, ü, ß) with at least two letters. Each line is
`word count`.

`exclude.txt` lists words that are never used, slurs and profanity, since the subtitle corpus
contains everything people say on screen. A trailing `*` excludes a prefix.

The lists are licensed CC BY-SA 4.0 (https://creativecommons.org/licenses/by-sa/4.0/), credit
Hermit Dave. They are used only to pick and weight words for generated lesson text.
