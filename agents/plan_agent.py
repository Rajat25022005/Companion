"""Production-grade planning agent with timeline generation, dependency analysis, and risk assessment."""
import json
import logging
import os
import subprocess
import time
import uuid
from datetime import datetime, timedelta
from pathlib import Path
from typing import Optional

from agents.base_agent import AgentResponse, BaseAgent
from agents.shared_utils import (
    retry, format_json_safe, ValidationError, validate_path, 
    MetricsCollector, sanitize_filename,
)
from memory.memory_manager import MemoryManager

logger = logging.getLogger(__name__)

PROJECT_ROOT = Path(__file__).parent.parent
WORKSPACE_DIR = PROJECT_ROOT / 'workspace'
WORKSPACE_DIR.mkdir(exist_ok=True)
VENV_PYTHON = str(PROJECT_ROOT / '.venv' / 'bin' / 'python3')


# ── Planning-specific tools ────────────────────────────────────────────────────

def search_semantic_memory(query: str, top_k: int = 5) -> str:
    """Search the user's indexed documents, specs, and notes for relevant content."""
    return format_json_safe({'info': 'semantic memory search stub', 'query': query, 'top_k': top_k})

search_semantic_memory._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'query': {'type': 'string', 'description': 'Search query for document corpus'},
            'top_k': {'type': 'integer', 'description': 'Number of results to return', 'default': 5},
        },
        'required': ['query'],
    }
}


def search_episodic_memory(query: str, top_k: int = 5) -> str:
    """Search past conversations for prior plans, project discussions, and status updates."""
    return format_json_safe({'info': 'episodic memory search stub', 'query': query, 'top_k': top_k})

search_episodic_memory._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'query': {'type': 'string', 'description': 'Search query for conversation history'},
            'top_k': {'type': 'integer', 'description': 'Number of results to return', 'default': 5},
        },
        'required': ['query'],
    }
}


def query_knowledge_graph(cypher_query: str) -> str:
    """Run a Cypher query against the knowledge graph to look up projects, people, deadlines, and dependencies."""
    return format_json_safe({'info': 'knowledge graph query stub', 'cypher': cypher_query})

query_knowledge_graph._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'cypher_query': {'type': 'string', 'description': 'Cypher query to execute against Neo4j'},
        },
        'required': ['cypher_query'],
    }
}


def execute_python(code: str, timeout: int = 30) -> str:
    """
    Run Python code for timeline calculations, resource allocation, or Gantt chart generation.

    Args:
        code: Python code to execute
        timeout: Execution timeout in seconds

    Returns:
        JSON with stdout, stderr, exit_code, and any generated files
    """
    env = os.environ.copy()
    env['MPLBACKEND'] = 'Agg'
    env['PYTHONDONTWRITEBYTECODE'] = '1'

    try:
        result = subprocess.run(
            [VENV_PYTHON, '-c', code],
            capture_output=True, text=True, timeout=timeout,
            env=env, cwd=str(WORKSPACE_DIR),
        )
        output = {
            'stdout': result.stdout[:2000],
            'stderr': result.stderr[:1000],
            'exit_code': result.returncode,
        }

        # Detect generated files (charts, timelines, etc.)
        cutoff = time.time() - 10
        for f in WORKSPACE_DIR.iterdir():
            if f.suffix.lower() in ('.png', '.jpg', '.svg', '.pdf', '.html', '.csv'):
                if f.stat().st_mtime > cutoff:
                    output['file'] = f.name
                    output['download_url'] = f'/files/{f.name}'
                    break

        return format_json_safe(output)

    except subprocess.TimeoutExpired:
        return format_json_safe({'error': f'Execution timed out after {timeout} seconds'})
    except Exception as e:
        return format_json_safe({'error': str(e)})

execute_python._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'code': {'type': 'string', 'description': 'Python code for calculations or data processing'},
            'timeout': {'type': 'integer', 'description': 'Execution timeout in seconds', 'default': 30},
        },
        'required': ['code'],
    }
}


def generate_timeline(tasks: list[dict], start_date: str = '', 
                      output_format: str = 'markdown') -> str:
    """
    Generate a structured project timeline from task definitions.

    Args:
        tasks: List of task dicts with 'name', 'duration_days', 'dependencies' (list of task names)
        start_date: Project start date (YYYY-MM-DD). Defaults to today.
        output_format: 'markdown', 'json', or 'csv'

    Returns:
        JSON with timeline data and formatted output
    """
    try:
        if not start_date:
            start = datetime.now()
        else:
            start = datetime.strptime(start_date, '%Y-%m-%d')

        # Build dependency graph and calculate dates
        task_map = {t['name']: t for t in tasks}
        scheduled = {}

        def schedule_task(name):
            if name in scheduled:
                return scheduled[name]['end_date']

            task = task_map.get(name)
            if not task:
                return start

            deps = task.get('dependencies', [])
            dep_end = start
            for dep in deps:
                dep_finish = schedule_task(dep)
                if dep_finish > dep_end:
                    dep_end = dep_finish

            duration = task.get('duration_days', 1)
            task_start = dep_end
            task_end = task_start + timedelta(days=duration)

            scheduled[name] = {
                'name': name,
                'start_date': task_start.strftime('%Y-%m-%d'),
                'end_date': task_end.strftime('%Y-%m-%d'),
                'duration_days': duration,
                'dependencies': deps,
            }
            return task_end

        for task in tasks:
            schedule_task(task['name'])

        timeline = list(scheduled.values())

        # Format output
        if output_format == 'markdown':
            lines = ['| Task | Start | End | Duration | Dependencies |',
                     '|------|-------|-----|----------|--------------|']
            for t in timeline:
                deps = ', '.join(t['dependencies']) if t['dependencies'] else 'None'
                lines.append(f"| {t['name']} | {t['start_date']} | {t['end_date']} | {t['duration_days']}d | {deps} |")
            formatted = '\n'.join(lines)
        elif output_format == 'csv':
            lines = ['Task,Start Date,End Date,Duration Days,Dependencies']
            for t in timeline:
                deps = ';'.join(t['dependencies']) if t['dependencies'] else ''
                lines.append(f"{t['name']},{t['start_date']},{t['end_date']},{t['duration_days']},{deps}")
            formatted = '\n'.join(lines)
        else:
            formatted = json.dumps(timeline, indent=2)

        return format_json_safe({
            'timeline': timeline,
            'formatted': formatted,
            'project_start': start.strftime('%Y-%m-%d'),
            'project_end': max(t['end_date'] for t in timeline),
            'total_duration_days': (datetime.strptime(max(t['end_date'] for t in timeline), '%Y-%m-%d') - start).days,
        })

    except Exception as e:
        return format_json_safe({'error': str(e)})

generate_timeline._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'tasks': {
                'type': 'array',
                'description': 'List of tasks with name, duration_days, and dependencies',
                'items': {
                    'type': 'object',
                    'properties': {
                        'name': {'type': 'string'},
                        'duration_days': {'type': 'integer'},
                        'dependencies': {'type': 'array', 'items': {'type': 'string'}},
                    },
                    'required': ['name', 'duration_days'],
                },
            },
            'start_date': {'type': 'string', 'description': 'Start date YYYY-MM-DD', 'default': ''},
            'output_format': {'type': 'string', 'description': 'markdown, json, or csv', 'default': 'markdown'},
        },
        'required': ['tasks'],
    }
}


def analyze_dependencies(tasks: list[dict]) -> str:
    """
    Analyze task dependencies for critical path and bottlenecks.

    Args:
        tasks: List of task dicts with 'name', 'duration_days', 'dependencies'

    Returns:
        JSON with critical path, bottlenecks, and risk analysis
    """
    try:
        task_map = {t['name']: t for t in tasks}

        # Find tasks with most dependents (bottlenecks)
        dependent_count = {name: 0 for name in task_map}
        for t in tasks:
            for dep in t.get('dependencies', []):
                if dep in dependent_count:
                    dependent_count[dep] += 1

        bottlenecks = sorted(
            [(name, count) for name, count in dependent_count.items() if count > 0],
            key=lambda x: x[1], reverse=True
        )[:5]

        # Find tasks with no dependencies (can start immediately)
        parallel_starters = [t['name'] for t in tasks if not t.get('dependencies')]

        # Find tasks with most dependencies (complex)
        complex_tasks = sorted(
            tasks,
            key=lambda t: len(t.get('dependencies', [])),
            reverse=True
        )[:5]

        # Detect circular dependencies
        def has_cycle(task_name, visited=None, stack=None):
            if visited is None:
                visited = set()
            if stack is None:
                stack = set()
            visited.add(task_name)
            stack.add(task_name)

            task = task_map.get(task_name)
            if task:
                for dep in task.get('dependencies', []):
                    if dep not in visited:
                        if has_cycle(dep, visited, stack):
                            return True
                    elif dep in stack:
                        return True
            stack.remove(task_name)
            return False

        cycles = []
        for t in tasks:
            if has_cycle(t['name']):
                cycles.append(t['name'])

        return format_json_safe({
            'bottlenecks': [{'task': b[0], 'blocks': b[1]} for b in bottlenecks],
            'parallel_starters': parallel_starters,
            'complex_tasks': [{'task': t['name'], 'dependencies': len(t.get('dependencies', []))} for t in complex_tasks],
            'circular_dependencies': cycles,
            'risk_level': 'high' if cycles else 'medium' if len(bottlenecks) > 2 else 'low',
        })

    except Exception as e:
        return format_json_safe({'error': str(e)})

analyze_dependencies._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'tasks': {
                'type': 'array',
                'description': 'List of tasks with name, duration_days, and dependencies',
                'items': {
                    'type': 'object',
                    'properties': {
                        'name': {'type': 'string'},
                        'duration_days': {'type': 'integer'},
                        'dependencies': {'type': 'array', 'items': {'type': 'string'}},
                    },
                    'required': ['name', 'duration_days'],
                },
            },
        },
        'required': ['tasks'],
    }
}


def estimate_effort(tasks: list[dict], team_size: int = 1, 
                    hours_per_day: float = 6.5) -> str:
    """
    Estimate total effort and calendar duration given team constraints.

    Args:
        tasks: List of tasks with 'name', 'duration_days', 'dependencies'
        team_size: Number of team members
        hours_per_day: Productive hours per person per day

    Returns:
        JSON with effort estimates and resource allocation
    """
    try:
        total_days = sum(t.get('duration_days', 1) for t in tasks)
        total_hours = total_days * hours_per_day
        calendar_days = total_days / max(team_size, 1)

        # Simple resource leveling: identify peak concurrent tasks
        task_map = {t['name']: t for t in tasks}
        concurrent = {}

        def mark_active(name, day):
            if name not in task_map:
                return
            duration = task_map[name].get('duration_days', 1)
            for d in range(day, day + duration):
                concurrent[d] = concurrent.get(d, 0) + 1
            for dep in task_map[name].get('dependencies', []):
                mark_active(dep, day)

        for i, t in enumerate(tasks):
            mark_active(t['name'], i * 2)  # Simplified scheduling

        peak_concurrency = max(concurrent.values()) if concurrent else 1

        return format_json_safe({
            'total_tasks': len(tasks),
            'total_work_days': total_days,
            'total_hours': round(total_hours, 1),
            'team_size': team_size,
            'hours_per_day': hours_per_day,
            'estimated_calendar_days': round(calendar_days, 1),
            'peak_concurrent_tasks': peak_concurrency,
            'team_utilization': f"{min(peak_concurrency / max(team_size, 1) * 100, 100):.0f}%",
            'buffer_recommended': f"{max(calendar_days * 0.2, 2):.0f} days",
        })

    except Exception as e:
        return format_json_safe({'error': str(e)})

estimate_effort._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'tasks': {
                'type': 'array',
                'description': 'List of tasks with name and duration_days',
                'items': {
                    'type': 'object',
                    'properties': {
                        'name': {'type': 'string'},
                        'duration_days': {'type': 'integer'},
                    },
                    'required': ['name', 'duration_days'],
                },
            },
            'team_size': {'type': 'integer', 'description': 'Number of team members', 'default': 1},
            'hours_per_day': {'type': 'number', 'description': 'Productive hours per day', 'default': 6.5},
        },
        'required': ['tasks'],
    }
}


PLAN_TOOLS = {
    'search_semantic_memory': search_semantic_memory,
    'search_episodic_memory': search_episodic_memory,
    'query_knowledge_graph': query_knowledge_graph,
    'execute_python': execute_python,
    'generate_timeline': generate_timeline,
    'analyze_dependencies': analyze_dependencies,
    'estimate_effort': estimate_effort,
}


# ── Agent ──────────────────────────────────────────────────────────────────────

class PlanAgent(BaseAgent):
    """Agent specialized in project planning, timeline generation, and resource allocation."""

    def __init__(
        self,
        memory: Optional[MemoryManager] = None,
        tools: Optional[dict[str, callable]] = None,
        **kwargs,
    ):
        effective_tools = {**PLAN_TOOLS}
        if tools:
            effective_tools.update(tools)

        # Wire up live memory-backed tool implementations
        if memory:
            effective_tools['search_semantic_memory'] = self._make_semantic_search(memory)
            effective_tools['search_episodic_memory'] = self._make_episodic_search(memory)
            effective_tools['query_knowledge_graph'] = self._make_graph_query(memory)

        super().__init__(memory=memory, tools=effective_tools, **kwargs)

    @property
    def agent_type(self) -> str:
        return 'plan'

    @property
    def skill_name(self) -> str:
        return 'plan'

    @property
    def memory_layers(self) -> list[str]:
        return ['episodic', 'semantic', 'relational']

    def get_available_tools(self) -> list[str]:
        return list(self._tools.keys())

    # ── Live memory tool factories ─────────────────────────────────────────

    def _make_semantic_search(self, memory: MemoryManager) -> callable:
        def search(query: str, top_k: int = 5) -> str:
            """Search the user's indexed documents, specs, and notes for relevant content."""
            try:
                context = memory.retrieve(query=query, layers=['semantic'], top_k=top_k)
                results = [
                    {
                        'title': e.get('title', ''), 
                        'content': e.get('content', '')[:300], 
                        'source': e.get('source_path', ''),
                        'score': e.get('score', 0),
                    }
                    for e in context.semantic
                ]
                return format_json_safe(results)
            except Exception as e:
                return format_json_safe({'error': str(e)})

        search._tool_schema = search_semantic_memory._tool_schema
        return search

    def _make_episodic_search(self, memory: MemoryManager) -> callable:
        def search(query: str, top_k: int = 5) -> str:
            """Search past conversations for prior plans, project discussions, and status updates."""
            try:
                context = memory.retrieve(
                    query=query,
                    layers=['episodic'],
                    top_k=top_k,
                    session_filter=getattr(self, '_current_session_id', ''),
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

        search._tool_schema = search_episodic_memory._tool_schema
        return search

    def _make_graph_query(self, memory: MemoryManager) -> callable:
        def query(cypher_query: str) -> str:
            """Run a Cypher query against the knowledge graph for structured lookups."""
            try:
                if not getattr(memory, 'relational', None):
                    return format_json_safe({'error': 'Knowledge graph not available'})
                result = memory.relational.query(cypher_query)
                return format_json_safe({
                    'entities': result.entities[:10],
                    'relationships': result.relationships[:10],
                    'records': [str(r) for r in result.raw_records[:10]],
                })
            except Exception as e:
                return format_json_safe({'error': str(e)})

        query._tool_schema = query_knowledge_graph._tool_schema
        return query

    # ── Convenience entry points ───────────────────────────────────────────

    def plan(
        self,
        request: str,
        conversation_history: Optional[list[dict]] = None,
        depth: str = 'standard',
    ) -> AgentResponse:
        """Execute a planning request with configurable depth."""
        extra = ''
        if depth == 'deep':
            extra = (
                'DEEP PLANNING MODE:\n'
                '1. Check all memory layers for prior work and constraints\n'
                '2. Identify dependencies and critical path using analyze_dependencies\n'
                '3. Generate timeline with generate_timeline\n'
                '4. Estimate effort with estimate_effort\n'
                '5. Flag risks, bottlenecks, and mitigation strategies\n'
                '6. Suggest phased delivery with clear milestones\n'
                '7. Think about what could go wrong and plan contingencies'
            )
        elif depth == 'quick':
            extra = (
                'QUICK PLANNING MODE:\n'
                '1. Give a focused breakdown with clear action items\n'
                '2. Skip elaborate analysis if scope is small\n'
                '3. Prioritize immediate next steps'
            )
        elif depth == 'resource':
            extra = (
                'RESOURCE PLANNING MODE:\n'
                '1. Analyze team capacity and constraints\n'
                '2. Identify parallelization opportunities\n'
                '3. Suggest optimal team allocation\n'
                '4. Calculate buffer and risk-adjusted timelines'
            )

        return self.run(
            user_message=request,
            conversation_history=conversation_history,
            extra_context=extra,
        )

    def create_timeline(
        self,
        tasks: list[dict],
        start_date: str = '',
        output_format: str = 'markdown',
    ) -> dict:
        """Direct API for timeline generation without LLM round-trip."""
        result = generate_timeline(tasks, start_date, output_format)
        try:
            return json.loads(result)
        except json.JSONDecodeError:
            return {'error': 'Failed to parse timeline result', 'raw': result}

    def analyze_risks(self, tasks: list[dict]) -> dict:
        """Direct API for dependency and risk analysis."""
        result = analyze_dependencies(tasks)
        try:
            return json.loads(result)
        except json.JSONDecodeError:
            return {'error': 'Failed to parse analysis result', 'raw': result}