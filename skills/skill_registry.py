# companion/skills/skill_registry.py

from pathlib import Path
from typing import Optional


SKILLS_DIR = Path(__file__).parent

SKILL_NAMES = [
    'research',
    'code',
    'write',
    'document',
    'email',
    'plan',
    'frontend_design',
    'file_reading',
]


def load_skill(skill_name: str) -> str:
    """Load a skill file by name and return its content as a string.

    Args:
        skill_name: Name of the skill (without .md extension).

    Returns:
        The skill file content, or an empty string if not found.
    """
    path = SKILLS_DIR / f'{skill_name}.md'
    return path.read_text(encoding='utf-8') if path.exists() else ''


def load_all_skills() -> dict[str, str]:
    """Load all registered skill files.

    Returns:
        A dictionary mapping skill names to their content.
    """
    return {name: load_skill(name) for name in SKILL_NAMES if load_skill(name)}


def get_skill_metadata(skill_name: str) -> Optional[dict]:
    """Parse a skill file and extract structured metadata.

    Extracts the model name, role description, and section headers
    from a skill markdown file.

    Args:
        skill_name: Name of the skill (without .md extension).

    Returns:
        A dictionary with parsed metadata, or None if the skill doesn't exist.
    """
    content = load_skill(skill_name)
    if not content:
        return None

    metadata = {
        'name': skill_name,
        'model': None,
        'role': None,
        'sections': [],
    }

    lines = content.split('\n')
    current_section = None

    for i, line in enumerate(lines):
        stripped = line.strip()

        if stripped.startswith('## Model'):
            next_line = lines[i + 1].strip() if i + 1 < len(lines) else ''
            metadata['model'] = next_line

        elif stripped.startswith('## Role'):
            role_lines = []
            for j in range(i + 1, len(lines)):
                if lines[j].strip().startswith('## '):
                    break
                if lines[j].strip():
                    role_lines.append(lines[j].strip())
            metadata['role'] = ' '.join(role_lines)

        elif stripped.startswith('## '):
            section_name = stripped[3:].strip()
            metadata['sections'].append(section_name)

    return metadata


def list_available_skills() -> list[dict]:
    """List all available skills with their metadata.

    Returns:
        A list of metadata dictionaries for each available skill.
    """
    skills = []
    for name in SKILL_NAMES:
        meta = get_skill_metadata(name)
        if meta:
            skills.append(meta)
    return skills
