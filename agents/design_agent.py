import os
import logging
import uuid
import json
from pathlib import Path
from typing import Optional

from agents.base_agent import AgentResponse, BaseAgent
from memory.memory_manager import MemoryManager

logger = logging.getLogger(__name__)

PROJECT_ROOT = Path(__file__).parent.parent
WORKSPACE_DIR = PROJECT_ROOT / 'workspace'

def generate_image(prompt: str) -> str:
    try:
        from google import genai
        from google.genai import types
        
        api_key = os.environ.get('GEMINI_API_KEY')
        if not api_key or api_key == 'your_key_here':
            return json.dumps({'error': 'GEMINI_API_KEY is not set in the .env file.'})
            
        # Load image model from config
        from core.conductor import CONFIG_DIR
        import yaml
        model_name = 'imagen-3.0-generate-002'
        try:
            with open(CONFIG_DIR / 'models.yaml', 'r') as f:
                config = yaml.safe_load(f)
                model_name = config.get('design', {}).get('image_model', model_name)
        except:
            pass

        client = genai.Client(api_key=api_key)
        
        logger.info(f'Generating image with model {model_name} and prompt: {prompt}')
        
        response = client.models.generate_images(
            model=model_name,
            prompt=prompt,
            config=types.GenerateImagesConfig(
                number_of_images=1,
                include_rai_reason=True,
            )
        )
        
        if not response.generated_images:
            return json.dumps({'error': 'No images were generated. It might have been blocked by safety filters.'})
            
        # Ensure workspace exists
        WORKSPACE_DIR.mkdir(parents=True, exist_ok=True)
        
        filename = f'generated_{uuid.uuid4().hex[:8]}.png'
        filepath = WORKSPACE_DIR / filename
        
        # response.generated_images[0].image is a PIL Image object
        image = response.generated_images[0].image
        image.save(filepath)
        
        return json.dumps({
            'status': 'success',
            'filename': filename,
            'local_path': str(filepath),
            'url': f'/files/{filename}',
            'prompt_used': prompt
        })
        
    except Exception as e:
        logger.error(f'Image generation failed: {e}')
        return json.dumps({'error': str(e)})

generate_image._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'prompt': {'type': 'string', 'description': 'A highly descriptive visual prompt for the image generator.'},
        },
        'required': ['prompt'],
    }
}

class DesignAgent(BaseAgent):
    def __init__(
        self,
        memory: Optional[MemoryManager] = None,
        **kwargs,
    ):
        tools = {'generate_image': generate_image}
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
            try:
                context = memory.retrieve(
                    query=query, 
                    layers=['episodic'], 
                    top_k=top_k,
                    session_filter=getattr(self, '_current_session_id', '')
                )
                results = [
                    {'content': e.get('content', '')[:200], 'response': e.get('response', '')[:200], 'timestamp': e.get('timestamp', '')}
                    for e in context.episodic
                ]
                return json.dumps(results, default=str)
            except Exception as e:
                return json.dumps({'error': str(e)})

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
            try:
                context = memory.retrieve(query=query, layers=['semantic'], top_k=top_k)
                results = [
                    {'title': e.get('title', ''), 'content': e.get('content', '')[:400], 'source': e.get('source_path', '')}
                    for e in context.semantic
                ]
                return json.dumps(results, default=str)
            except Exception as e:
                return json.dumps({'error': str(e)})

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
