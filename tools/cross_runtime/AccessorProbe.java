/*
 * Copyright (c) 2021-2024 Works Applications Co., Ltd.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

import com.worksap.nlp.sudachi.Config;
import com.worksap.nlp.sudachi.Dictionary;
import com.worksap.nlp.sudachi.DictionaryFactory;
import com.worksap.nlp.sudachi.Morpheme;
import com.worksap.nlp.sudachi.PathAnchor;
import com.worksap.nlp.sudachi.Tokenizer;

import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.List;

public class AccessorProbe {
    private static class Args {
        Path config;
        Path resourceDir;
        Path dictionary;
        Path input;
        Path output;
        Tokenizer.SplitMode mode = Tokenizer.SplitMode.C;
    }

    public static void main(String[] argv) throws Exception {
        Args args = parseArgs(argv);
        Config config = args.resourceDir == null
                ? Config.fromFile(args.config)
                : Config.fromFile(args.config, PathAnchor.filesystem(args.resourceDir));
        if (args.dictionary != null) {
            config.systemDictionary(args.dictionary);
        }

        try (Dictionary dictionary = new DictionaryFactory().create(config);
                BufferedReader reader = Files.newBufferedReader(args.input, StandardCharsets.UTF_8);
                BufferedWriter writer = args.output == null
                        ? new BufferedWriter(new java.io.OutputStreamWriter(System.out, StandardCharsets.UTF_8))
                        : Files.newBufferedWriter(args.output, StandardCharsets.UTF_8)) {
            Tokenizer tokenizer = dictionary.create();
            String line;
            int lineIdx = 0;
            while ((line = reader.readLine()) != null) {
                List<Morpheme> morphemes;
                try {
                    morphemes = tokenizer.tokenize(args.mode, line);
                } catch (RuntimeException e) {
                    throw new IllegalStateException(
                            "failed to tokenize line " + lineIdx + ": " + preview(line), e);
                }
                for (int morphIdx = 0; morphIdx < morphemes.size(); morphIdx++) {
                    Morpheme morpheme = morphemes.get(morphIdx);
                    writeRow(writer, lineIdx, morphIdx, line, morpheme);
                }
                lineIdx++;
            }
        }
    }

    private static String preview(String line) {
        int maxCodePoints = 80;
        if (line.codePointCount(0, line.length()) <= maxCodePoints) {
            return line;
        }
        int end = line.offsetByCodePoints(0, maxCodePoints);
        return line.substring(0, end) + "...";
    }

    private static void writeRow(BufferedWriter writer, int lineIdx, int morphIdx, String line, Morpheme morpheme)
            throws IOException {
        writer.write(Integer.toString(lineIdx));
        writer.write('\t');
        writer.write(Integer.toString(morphIdx));
        writer.write('\t');
        writer.write(Integer.toString(line.codePointCount(0, morpheme.begin())));
        writer.write('\t');
        writer.write(Integer.toString(line.codePointCount(0, morpheme.end())));
        writer.write('\t');
        writer.write(escaped(morpheme.surface()));
        writer.write('\t');
        writer.write(Long.toString(Integer.toUnsignedLong(morpheme.getWordId())));
        writer.write('\t');
        writer.write(Integer.toString(morpheme.getDictionaryId()));
        writer.write('\t');
        writer.write(Integer.toString(Short.toUnsignedInt(morpheme.partOfSpeechId())));
        writer.write('\t');
        writer.write(escaped(morpheme.dictionaryForm()));
        writer.write('\t');
        writer.write(escaped(morpheme.normalizedForm()));
        writer.write('\t');
        writer.write(formatSplit(morpheme, Tokenizer.SplitMode.A));
        writer.write('\t');
        writer.write(formatSplit(morpheme, Tokenizer.SplitMode.B));
        writer.write('\n');
    }

    private static String formatSplit(Morpheme original, Tokenizer.SplitMode mode) {
        List<Morpheme> split = original.split(mode);
        if (isNoSplit(original, split)) {
            return "";
        }

        StringBuilder result = new StringBuilder();
        for (int i = 0; i < split.size(); i++) {
            if (i > 0) {
                result.append('|');
            }
            Morpheme morpheme = split.get(i);
            result.append(escaped(morpheme.surface()));
            result.append(':');
            result.append(Integer.toUnsignedLong(morpheme.getWordId()));
        }
        return result.toString();
    }

    private static boolean isNoSplit(Morpheme original, List<Morpheme> split) {
        if (split.isEmpty()) {
            return true;
        }
        if (split.size() != 1) {
            return false;
        }
        Morpheme only = split.get(0);
        return only.getWordId() == original.getWordId()
                && only.begin() == original.begin()
                && only.end() == original.end()
                && only.surface().equals(original.surface());
    }

    private static String escaped(String text) {
        StringBuilder result = new StringBuilder(text.length());
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            switch (c) {
                case '\\':
                    result.append("\\\\");
                    break;
                case '\t':
                    result.append("\\t");
                    break;
                case '\n':
                    result.append("\\n");
                    break;
                case '\r':
                    result.append("\\r");
                    break;
                default:
                    result.append(c);
                    break;
            }
        }
        return result.toString();
    }

    private static Args parseArgs(String[] argv) {
        Args args = new Args();
        Path root = Paths.get("").toAbsolutePath();
        args.config = root.resolve("resources").resolve("sudachi.json");
        args.resourceDir = root.resolve("resources");

        for (int i = 0; i < argv.length; i++) {
            String arg = argv[i];
            switch (arg) {
                case "--config":
                case "--config-file":
                    args.config = Paths.get(value(argv, ++i, arg));
                    break;
                case "--resource-dir":
                    args.resourceDir = Paths.get(value(argv, ++i, arg));
                    break;
                case "--dict":
                    args.dictionary = Paths.get(value(argv, ++i, arg));
                    break;
                case "--input":
                    args.input = Paths.get(value(argv, ++i, arg));
                    break;
                case "--output":
                    args.output = Paths.get(value(argv, ++i, arg));
                    break;
                case "--mode":
                    args.mode = Tokenizer.SplitMode.valueOf(value(argv, ++i, arg).toUpperCase());
                    break;
                case "--help":
                case "-h":
                    usageAndExit();
                    break;
                default:
                    throw new IllegalArgumentException("unknown argument: " + arg);
            }
        }

        if (args.input == null) {
            throw new IllegalArgumentException("--input is required");
        }
        return args;
    }

    private static String value(String[] argv, int idx, String arg) {
        if (idx >= argv.length) {
            throw new IllegalArgumentException(arg + " requires a value");
        }
        return argv[idx];
    }

    private static void usageAndExit() {
        System.err.println("Usage: java AccessorProbe --input PATH [--output PATH] "
                + "[--config PATH] [--resource-dir PATH] [--dict PATH] [--mode A|B|C]");
        System.exit(2);
    }
}
