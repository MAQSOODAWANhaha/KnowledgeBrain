#!/usr/bin/env python3
"""Launch the local real Agent acceptance runner from one explicit .env file.

Provider settings come exclusively from that file. No model override/fallback,
no shell evaluation and no credential printing. Budgets are a separate explicit
JSON input, frozen by the Rust runner.
"""
import argparse
import json
import os
from pathlib import Path


def provider_environment(env_file, inherited):
    from dotenv import dotenv_values
    if not env_file.is_file():
        raise ValueError('explicit env file does not exist')
    config = dotenv_values(env_file, interpolate=False)
    keys = ('KNOWLEDGEBRAIN_CHAT_BASE_URL', 'KNOWLEDGEBRAIN_CHAT_API_KEY',
            'KNOWLEDGEBRAIN_CHAT_MODEL', 'LLM_BASE_URL', 'LLM_API_KEY', 'LLM_MODEL',
            'KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT', 'KB_AUTHORING_MAX_OUTPUT_TOKENS',
            'KB_AUTHORING_TIMEOUT_MS')
    env = {k: v for k, v in inherited.items() if k not in keys}
    for key in keys:
        value = config.get(key)
        if value:
            if '${' in value:
                raise ValueError(f'{key}: unresolved interpolation in explicit config')
            env[key] = value
    if not (env.get('KNOWLEDGEBRAIN_CHAT_MODEL') or env.get('LLM_MODEL')):
        raise ValueError('explicit env file has no configured model')
    return env


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--env-file', required=True, type=Path)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--source-directory', required=True, type=Path)
    parser.add_argument('--limits', required=True, type=Path)
    parser.add_argument('--run-directory', required=True, type=Path)
    parser.add_argument('--mode', required=True, choices=['extract', 'review', 'compose'])
    args = parser.parse_args()
    try:
        env = provider_environment(args.env_file, os.environ)
    except ValueError as error:
        raise SystemExit(str(error)) from error
    model = env.get('KNOWLEDGEBRAIN_CHAT_MODEL') or env.get('LLM_MODEL')
    print(json.dumps({'env_file': str(args.env_file.resolve()), 'model': model,
                      'mode': args.mode}), flush=True)
    binary = str(args.binary.resolve())
    os.execve(binary, [binary, args.mode, str(args.source_directory.resolve()),
                       str(args.limits.resolve()), str(args.run_directory.resolve())], env)


if __name__ == '__main__':
    main()
