import json
import logging
import time
from typing import Any, Optional

from pydantic import BaseModel, Field

from agents.base_agent import AgentResponse
from agents.research_agent import ResearchAgent
from agents.code_agent import CodeAgent
from agents.writer_agent import WriterAgent
from agents.doc_agent import DocAgent
from core.intent_parser import Intent
from memory.memory_manager import MemoryManager

logger = logging.getLogger(__name__)


class TaskNode(BaseModel):
    step: int
    agent: str
    task: str
    depends_on: list[int] = Field(default_factory=list)
    status: str = 'pending'
    result: Optional[str] = None
    error: Optional[str] = None
    latency_ms: float = 0.0


class TaskGraph(BaseModel):
    nodes: list[TaskNode] = Field(default_factory=list)
    status: str = 'pending'
    total_latency_ms: float = 0.0


class PlannerResult(BaseModel):
    graph: TaskGraph
    final_output: str = ''
    agent_responses: list[dict] = Field(default_factory=list)


AGENT_REGISTRY = {
    'research': ResearchAgent,
    'code': CodeAgent,
    'write': WriterAgent,
    'document': DocAgent,
}


class TaskPlanner:
    def __init__(
        self,
        memory: Optional[MemoryManager] = None,
    ):
        self._memory = memory
        self._agents: dict[str, Any] = {}

    def _get_agent(self, agent_type: str):
        if agent_type not in self._agents:
            agent_cls = AGENT_REGISTRY.get(agent_type)
            if agent_cls is None:
                raise ValueError(f'Unknown agent type: {agent_type}. Available: {list(AGENT_REGISTRY.keys())}')
            self._agents[agent_type] = agent_cls(memory=self._memory)

        return self._agents[agent_type]

    def build_graph(self, intent: Intent) -> TaskGraph:
        if not intent.task_chain:
            return TaskGraph(
                nodes=[
                    TaskNode(step=1, agent=intent.primary, task='', depends_on=[])
                ]
            )

        nodes = []
        for task_def in intent.task_chain:
            nodes.append(
                TaskNode(
                    step=task_def.get('step', len(nodes) + 1),
                    agent=task_def.get('agent', intent.primary),
                    task=task_def.get('task', ''),
                    depends_on=task_def.get('depends_on', []),
                )
            )

        return TaskGraph(nodes=nodes)

    def _get_execution_order(self, graph: TaskGraph) -> list[list[TaskNode]]:
        node_map = {node.step: node for node in graph.nodes}
        completed = set()
        levels = []

        remaining = set(node_map.keys())

        while remaining:
            current_level = []
            for step in list(remaining):
                node = node_map[step]
                deps_met = all(d in completed for d in node.depends_on)
                if deps_met:
                    current_level.append(node)

            if not current_level:
                logger.error('Circular dependency detected in task graph.')
                for step in remaining:
                    current_level.append(node_map[step])
                levels.append(current_level)
                break

            levels.append(current_level)
            for node in current_level:
                remaining.discard(node.step)
                completed.add(node.step)

        return levels

    def execute(
        self,
        intent: Intent,
        user_message: str,
        conversation_history: Optional[list[dict]] = None,
    ) -> PlannerResult:
        graph = self.build_graph(intent)
        start = time.monotonic()
        agent_responses = []
        node_results: dict[int, str] = {}

        execution_levels = self._get_execution_order(graph)

        for level in execution_levels:
            for node in level:
                node.status = 'running'

                dep_context = ''
                if node.depends_on:
                    dep_parts = []
                    for dep_step in node.depends_on:
                        if dep_step in node_results:
                            dep_parts.append(
                                f'--- Result from step {dep_step} ---\n{node_results[dep_step]}\n--- End step {dep_step} ---'
                            )
                    dep_context = '\n\n'.join(dep_parts)

                task_message = node.task if node.task else user_message

                if dep_context:
                    task_message = f'{task_message}\n\nContext from previous steps:\n{dep_context}'

                try:
                    agent = self._get_agent(node.agent)

                    node_start = time.monotonic()
                    response = agent.run(
                        user_message=task_message,
                        conversation_history=conversation_history,
                    )
                    node.latency_ms = (time.monotonic() - node_start) * 1000

                    node.result = response.content
                    node.status = 'done'
                    node_results[node.step] = response.content

                    agent_responses.append({
                        'step': node.step,
                        'agent': node.agent,
                        'content': response.content,
                        'tool_calls': response.tool_calls,
                        'model': response.model,
                        'latency_ms': node.latency_ms,
                    })

                    logger.info(
                        'Step %d (%s) completed in %.0fms.',
                        node.step, node.agent, node.latency_ms,
                    )

                except Exception as e:
                    node.status = 'failed'
                    node.error = str(e)
                    node_results[node.step] = f'[ERROR: {e}]'
                    logger.error('Step %d (%s) failed: %s', node.step, node.agent, e)

        graph.total_latency_ms = (time.monotonic() - start) * 1000
        graph.status = 'done' if all(n.status == 'done' for n in graph.nodes) else 'partial'

        final_node = max(graph.nodes, key=lambda n: n.step)
        final_output = final_node.result or ''

        return PlannerResult(
            graph=graph,
            final_output=final_output,
            agent_responses=agent_responses,
        )

    def execute_single(
        self,
        agent_type: str,
        message: str,
        conversation_history: Optional[list[dict]] = None,
    ) -> AgentResponse:
        agent = self._get_agent(agent_type)
        return agent.run(
            user_message=message,
            conversation_history=conversation_history,
        )
