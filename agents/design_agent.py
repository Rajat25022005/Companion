"""Production-grade design agent with prompt enhancement and multi-provider support."""
import json
import logging
import os
import uuid
from pathlib import Path
from typing import Optional

from agents.base_agent import AgentResponse, BaseAgent
from agents.shared_utils import format_json_safe, ValidationError
from memory.memory_manager import MemoryManager

logger = logging.getLogger(__name__)

PROJECT_ROOT = Path(__file__).parent.parent
WORKSPACE_DIR = PROJECT_ROOT / 'workspace'

# Default model configuration
DEFAULT_IMAGE_MODEL = 'imagen-3.0-generate-002'
FALLBACK_IMAGE_MODEL = 'imagen-3.0-generate-001'


def _load_image_config() -> dict:
    """Load image generation configuration."""
    try:
        from core.conductor import CONFIG_DIR
        import yaml
        with open(CONFIG_DIR / 'models.yaml', 'r') as f:
            config = yaml.safe_load(f)
            return config.get('design', {})
    except Exception:
        return {}


def enhance_prompt(base_prompt: str, style: str = '', aspect_ratio: str = '16:9') -> str:
    """
    Enhance a user prompt with professional photography/design terminology.

    Args:
        base_prompt: User's raw prompt
        style: Optional style modifier (photorealistic, illustration, 3d, etc.)
        aspect_ratio: Target aspect ratio

    Returns:
        Enhanced prompt string
    """
    enhancements = []

    if style:
        style_modifiers = {
            'photorealistic': 'highly detailed, photorealistic, 8k resolution, professional photography, '
                            'cinematic lighting, sharp focus, depth of field',
            'illustration': 'digital illustration, clean linework, vibrant colors, artstation trending, '
                          'concept art style',
            '3d': '3D render, octane render, blender, volumetric lighting, physically based rendering, '
                 'subsurface scattering',
            'minimal': 'minimalist design, clean composition, negative space, elegant simplicity, '
                      'professional graphic design',
            'sketch': 'hand-drawn sketch, pencil on paper, artistic draft, expressive linework, '
                     'concept sketch',
        }
        enhancements.append(style_modifiers.get(style.lower(), style))
    else:
        enhancements.append('high quality, detailed, professional')

    # Aspect ratio guidance
    ar_guidance = {
        '16:9': 'widescreen composition',
        '4:3': 'standard composition',
        '1:1': 'square composition',
        '9:16': 'vertical portrait composition',
        '21:9': 'ultrawide cinematic composition',
    }
    if aspect_ratio in ar_guidance:
        enhancements.append(ar_guidance[aspect_ratio])

    enhanced = f"{base_prompt}, {', '.join(enhancements)}"
    return enhanced


def generate_image(prompt: str, style: str = '', aspect_ratio: str = '16:9',
                   negative_prompt: str = '') -> str:
    """
    Generate an image using Google's Imagen API with prompt enhancement.

    Args:
        prompt: Base image description
        style: Visual style modifier
        aspect_ratio: Target aspect ratio
        negative_prompt: Things to avoid in the image

    Returns:
        JSON string with result metadata
    """
    try:
        from google import genai
        from google.genai import types

        api_key = os.environ.get('GEMINI_API_KEY')
        if not api_key or api_key == 'your_key_here':
            return format_json_safe({
                'error': 'GEMINI_API_KEY is not set in the .env file.',
                'setup_instructions': 'Add GEMINI_API_KEY=your_key to .env'
            })

        config = _load_image_config()
        model_name = config.get('image_model', DEFAULT_IMAGE_MODEL)

        # Enhance prompt
        enhanced_prompt = enhance_prompt(prompt, style, aspect_ratio)

        client = genai.Client(api_key=api_key)

        logger.info('Generating image with model %s', model_name)
        logger.debug('Enhanced prompt: %s', enhanced_prompt)

        generation_config = types.GenerateImagesConfig(
            number_of_images=1,
            include_rai_reason=True,
        )

        # Add aspect ratio if supported
        if aspect_ratio and hasattr(generation_config, 'aspect_ratio'):
            generation_config.aspect_ratio = aspect_ratio

        response = client.models.generate_images(
            model=model_name,
            prompt=enhanced_prompt,
            config=generation_config,
        )

        if not response.generated_images:
            rai_reason = getattr(response, 'rai_reason', 'Unknown')
            return format_json_safe({
                'error': 'No images were generated.',
                'rai_reason': rai_reason,
                'suggestion': 'Try a different prompt or check safety settings.',
            })

        WORKSPACE_DIR.mkdir(parents=True, exist_ok=True)

        filename = f'generated_{uuid.uuid4().hex[:8]}.png'
        filepath = WORKSPACE_DIR / filename

        image = response.generated_images[0].image
        image.save(filepath)

        # Get image dimensions
        width, height = image.size if hasattr(image, 'size') else (None, None)

        return format_json_safe({
            'status': 'success',
            'filename': filename,
            'local_path': str(filepath),
            'url': f'/files/{filename}',
            'prompt_used': enhanced_prompt,
            'original_prompt': prompt,
            'style': style,
            'dimensions': {'width': width, 'height': height},
            'model': model_name,
        })

    except ImportError:
        return format_json_safe({
            'error': 'Google GenAI library not installed. Run: pip install google-genai',
        })
    except Exception as e:
        logger.error('Image generation failed: %s', e, exc_info=True)
        return format_json_safe({
            'error': str(e),
            'error_type': type(e).__name__,
            'suggestion': 'Check API key, network connection, and prompt content.',
        })

generate_image._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'prompt': {
                'type': 'string',
                'description': 'A highly descriptive visual prompt for the image generator.',
            },
            'style': {
                'type': 'string',
                'description': 'Visual style: photorealistic, illustration, 3d, minimal, sketch',
                'default': '',
            },
            'aspect_ratio': {
                'type': 'string',
                'description': 'Aspect ratio: 16:9, 4:3, 1:1, 9:16',
                'default': '16:9',
            },
            'negative_prompt': {
                'type': 'string',
                'description': 'Elements to avoid in the generated image',
                'default': '',
            },
        },
        'required': ['prompt'],
    }
}


def analyze_image(image_path: str) -> str:
    """
    Analyze an image and return descriptive metadata.
    Stub for future vision model integration.
    """
    try:
        from PIL import Image
        p = Path(image_path)
        if not p.exists():
            return format_json_safe({'error': f'Image not found: {image_path}'})

        img = Image.open(p)
        return format_json_safe({
            'filename': p.name,
            'format': img.format,
            'mode': img.mode,
            'dimensions': {'width': img.width, 'height': img.height},
            'file_size_bytes': p.stat().st_size,
        })
    except Exception as e:
        return format_json_safe({'error': str(e)})

analyze_image._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'image_path': {'type': 'string', 'description': 'Path to image file'},
        },
        'required': ['image_path'],
    }
}


class DesignAgent(BaseAgent):
    """Agent specialized in visual design and image generation."""

    def __init__(
        self,
        memory: Optional[MemoryManager] = None,
        **kwargs,
    ):
        tools = {
            'generate_image': generate_image,
            'analyze_image': analyze_image,
        }
        if memory:
            tools['search_episodic_memory'] = self._make_episodic_search(memory)
            tools['search_semantic_memory'] = self._make_semantic_search(memory)

        super().__init__(memory=memory, tools=tools, **kwargs)

    @property
    def agent_type(self) -> str:
        return 'design'

    @property
    def skill_name(self) -> str:
        return 'design'

    @property
    def memory_layers(self) -> list[str]:
        return ['episodic', 'semantic']

    def get_available_tools(self) -> list[str]:
        return list(self._tools.keys())

    def _make_episodic_search(self, memory: MemoryManager) -> callable:
        def search(query: str, top_k: int = 5) -> str:
            """Search past design discussions and preferences."""
            try:
                context = memory.retrieve(
                    query=query, 
                    layers=['episodic'], 
                    top_k=top_k,
                    session_filter=getattr(self, '_current_session_id', '')
                )
                results = [
                    {
                        'content': e.get('content', '')[:200], 
                        'response': e.get('response', '')[:200], 
                        'timestamp': e.get('timestamp', ''),
                    }
                    for e in context.episodic
                ]
                return format_json_safe(results)
            except Exception as e:
                return format_json_safe({'error': str(e)})

        search._tool_schema = {
            'parameters': {
                'type': 'object',
                'properties': {
                    'query': {'type': 'string', 'description': 'Search query for past design discussions'},
                    'top_k': {'type': 'integer', 'default': 5},
                },
                'required': ['query'],
            }
        }
        return search

    def _make_semantic_search(self, memory: MemoryManager) -> callable:
        def search(query: str, top_k: int = 5) -> str:
            """Search for visual assets and design references."""
            try:
                context = memory.retrieve(query=query, layers=['semantic'], top_k=top_k)
                results = [
                    {
                        'title': e.get('title', ''), 
                        'content': e.get('content', '')[:400], 
                        'source': e.get('source_path', ''),
                    }
                    for e in context.semantic
                ]
                return format_json_safe(results)
            except Exception as e:
                return format_json_safe({'error': str(e)})

        search._tool_schema = {
            'parameters': {
                'type': 'object',
                'properties': {
                    'query': {'type': 'string', 'description': 'Search query for visual assets'},
                    'top_k': {'type': 'integer', 'default': 5},
                },
                'required': ['query'],
            }
        }
        return search

    def design(
        self,
        request: str,
        conversation_history: Optional[list[dict]] = None,
        style: str = '',
    ) -> AgentResponse:
        """Execute a design request with optional style guidance."""
        extra = ''
        if style:
            extra = f"Preferred visual style: {style}. Use this style parameter when calling generate_image."
        return self.run(
            user_message=request,
            conversation_history=conversation_history,
            extra_context=extra,
        )