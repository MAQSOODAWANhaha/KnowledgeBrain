"""Do not accidentally use inherited credentials/models from another .env."""
import importlib.util
from pathlib import Path

import pytest

spec = importlib.util.spec_from_file_location('sample_run', Path(__file__).parents[1] / 'bidding_sample_run.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def test_selected_env_is_reloaded_and_clears_inherited_provider(tmp_path):
    config = tmp_path / '.env'
    inherited = dict(PATH='/bin', KNOWLEDGEBRAIN_CHAT_MODEL='stale',
                     KNOWLEDGEBRAIN_CHAT_API_KEY='stale-secret',
                     KNOWLEDGEBRAIN_CHAT_BASE_URL='https://stale.invalid')
    config.write_text('LLM_MODEL=model-from-file\nLLM_API_KEY=test-only\nLLM_BASE_URL=https://configured.invalid\n')
    env = module.provider_environment(config, inherited)
    assert env['LLM_MODEL'] == 'model-from-file'
    assert 'KNOWLEDGEBRAIN_CHAT_MODEL' not in env
    assert 'KNOWLEDGEBRAIN_CHAT_API_KEY' not in env
    assert 'KNOWLEDGEBRAIN_CHAT_BASE_URL' not in env
    assert env['PATH'] == '/bin'
    config.write_text('KNOWLEDGEBRAIN_CHAT_MODEL=updated-in-file\n')
    assert module.provider_environment(config, inherited)['KNOWLEDGEBRAIN_CHAT_MODEL'] == 'updated-in-file'


def test_missing_model_or_interpolation_does_not_fall_back(tmp_path):
    config = tmp_path / '.env'
    config.write_text('LLM_API_KEY=test-only\n')
    with pytest.raises(ValueError, match='no configured model'):
        module.provider_environment(config, {'LLM_MODEL': 'inherited'})
    config.write_text('LLM_MODEL=${INHERITED_MODEL}\n')
    with pytest.raises(ValueError, match='unresolved interpolation'):
        module.provider_environment(config, {'INHERITED_MODEL': 'inherited'})


def test_tuning_is_reloaded_from_selected_file_without_inherited_reasoning(tmp_path):
    config = tmp_path / '.env'
    config.write_text('LLM_MODEL=configured\nKB_AUTHORING_MAX_OUTPUT_TOKENS=4096\n'
                      'KB_AUTHORING_TIMEOUT_MS=90000\n')
    inherited = dict(KB_AUTHORING_MAX_OUTPUT_TOKENS='99999', KB_AUTHORING_TIMEOUT_MS='1',
                     KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT='stale')
    env = module.provider_environment(config, inherited)
    assert env['KB_AUTHORING_MAX_OUTPUT_TOKENS'] == '4096'
    assert env['KB_AUTHORING_TIMEOUT_MS'] == '90000'
    assert 'KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT' not in env
    config.write_text(config.read_text() + 'KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT=high\n')
    assert module.provider_environment(config, inherited)['KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT'] == 'high'
