import logging
from datetime import datetime, timezone
from typing import Optional

from neo4j import GraphDatabase
from pydantic import BaseModel, Field
from tenacity import retry, stop_after_attempt, wait_exponential

logger = logging.getLogger(__name__)

NODE_TYPES = {'Person', 'Project', 'Concept', 'Tool', 'File', 'Event'}
EDGE_TYPES = {'WORKS_ON', 'USES', 'RELATED_TO', 'MENTIONED_WITH', 'CREATED_BY'}


class Entity(BaseModel):
    name: str
    entity_type: str
    properties: dict = Field(default_factory=dict)


class Relationship(BaseModel):
    source: str
    source_type: str
    target: str
    target_type: str
    relation: str
    properties: dict = Field(default_factory=dict)


class GraphResult(BaseModel):
    entities: list[dict] = Field(default_factory=list)
    relationships: list[dict] = Field(default_factory=list)
    raw_records: list[dict] = Field(default_factory=list)


class RelationalMemory:
    def __init__(
        self,
        uri: str = 'bolt://localhost:7687',
        user: str = 'neo4j',
        password: str = 'companion',
        database: str = 'neo4j',
    ):
        self._driver = GraphDatabase.driver(uri, auth=(user, password))
        self._database = database
        self._ensure_constraints()

    def _ensure_constraints(self) -> None:
        constraints = [
            'CREATE CONSTRAINT IF NOT EXISTS FOR (p:Person) REQUIRE p.name IS UNIQUE',
            'CREATE CONSTRAINT IF NOT EXISTS FOR (p:Project) REQUIRE p.name IS UNIQUE',
            'CREATE CONSTRAINT IF NOT EXISTS FOR (c:Concept) REQUIRE c.name IS UNIQUE',
            'CREATE CONSTRAINT IF NOT EXISTS FOR (t:Tool) REQUIRE t.name IS UNIQUE',
            'CREATE CONSTRAINT IF NOT EXISTS FOR (f:File) REQUIRE f.name IS UNIQUE',
            'CREATE CONSTRAINT IF NOT EXISTS FOR (e:Event) REQUIRE e.name IS UNIQUE',
        ]
        with self._driver.session(database=self._database) as session:
            for stmt in constraints:
                try:
                    session.run(stmt)
                except Exception as e:
                    logger.debug('Constraint may already exist: %s', e)

        indexes = [
            'CREATE INDEX IF NOT EXISTS FOR (n:Person) ON (n.updated_at)',
            'CREATE INDEX IF NOT EXISTS FOR (n:Project) ON (n.updated_at)',
        ]
        with self._driver.session(database=self._database) as session:
            for stmt in indexes:
                try:
                    session.run(stmt)
                except Exception as e:
                    logger.debug('Index may already exist: %s', e)

    def close(self) -> None:
        self._driver.close()

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        self.close()

    @retry(stop=stop_after_attempt(3), wait=wait_exponential(multiplier=0.5, max=5), reraise=True)
    def upsert_entity(self, entity: Entity) -> dict:
        if entity.entity_type not in NODE_TYPES:
            raise ValueError(f'Invalid entity type: {entity.entity_type}. Must be one of {NODE_TYPES}')

        now = datetime.now(timezone.utc).isoformat()
        props = {**entity.properties, 'updated_at': now}

        query = (
            f'MERGE (n:{entity.entity_type} {{name: $name}}) '
            f'ON CREATE SET n += $props, n.created_at = $now '
            f'ON MATCH SET n += $props '
            f'RETURN n'
        )

        with self._driver.session(database=self._database) as session:
            result = session.run(query, name=entity.name, props=props, now=now)
            record = result.single()
            if record:
                node = record['n']
                return dict(node)
        return {}

    def upsert_entities_batch(self, entities: list[Entity]) -> int:
        count = 0
        with self._driver.session(database=self._database) as session:
            for entity in entities:
                if entity.entity_type not in NODE_TYPES:
                    logger.warning('Skipping invalid entity type: %s', entity.entity_type)
                    continue
                now = datetime.now(timezone.utc).isoformat()
                props = {**entity.properties, 'updated_at': now}
                query = (
                    f'MERGE (n:{entity.entity_type} {{name: $name}}) '
                    f'ON CREATE SET n += $props, n.created_at = $now '
                    f'ON MATCH SET n += $props '
                )
                session.run(query, name=entity.name, props=props, now=now)
                count += 1
        return count

    @retry(stop=stop_after_attempt(3), wait=wait_exponential(multiplier=0.5, max=5), reraise=True)
    def add_relationship(self, rel: Relationship) -> dict:
        if rel.relation not in EDGE_TYPES:
            raise ValueError(f'Invalid relation: {rel.relation}. Must be one of {EDGE_TYPES}')
        if rel.source_type not in NODE_TYPES or rel.target_type not in NODE_TYPES:
            raise ValueError('Invalid source or target entity type.')

        now = datetime.now(timezone.utc).isoformat()
        props = {**rel.properties, 'updated_at': now}

        query = (
            f'MERGE (a:{rel.source_type} {{name: $source}}) '
            f'MERGE (b:{rel.target_type} {{name: $target}}) '
            f'MERGE (a)-[r:{rel.relation}]->(b) '
            f'ON CREATE SET r += $props, r.created_at = $now '
            f'ON MATCH SET r += $props '
            f'RETURN a.name AS source, type(r) AS relation, b.name AS target'
        )

        with self._driver.session(database=self._database) as session:
            result = session.run(
                query, source=rel.source, target=rel.target, props=props, now=now,
            )
            record = result.single()
            if record:
                return dict(record)
        return {}

    def add_relationships_batch(self, relationships: list[Relationship]) -> int:
        count = 0
        with self._driver.session(database=self._database) as session:
            for rel in relationships:
                if rel.relation not in EDGE_TYPES:
                    logger.warning('Skipping invalid relation: %s', rel.relation)
                    continue
                now = datetime.now(timezone.utc).isoformat()
                props = {**rel.properties, 'updated_at': now}
                query = (
                    f'MERGE (a:{rel.source_type} {{name: $source}}) '
                    f'MERGE (b:{rel.target_type} {{name: $target}}) '
                    f'MERGE (a)-[r:{rel.relation}]->(b) '
                    f'ON CREATE SET r += $props, r.created_at = $now '
                    f'ON MATCH SET r += $props '
                )
                session.run(query, source=rel.source, target=rel.target, props=props, now=now)
                count += 1
        return count

    @retry(stop=stop_after_attempt(3), wait=wait_exponential(multiplier=0.5, max=5), reraise=True)
    def query(self, cypher: str, params: Optional[dict] = None) -> GraphResult:
        params = params or {}
        with self._driver.session(database=self._database) as session:
            result = session.run(cypher, **params)
            records = [dict(r) for r in result]

        entities = []
        relationships = []
        for record in records:
            for val in record.values():
                if hasattr(val, 'labels'):
                    entities.append({
                        'name': val.get('name', ''),
                        'type': list(val.labels)[0] if val.labels else '',
                        'properties': dict(val),
                    })

        return GraphResult(entities=entities, relationships=relationships, raw_records=records)

    def get_entity(self, name: str, entity_type: Optional[str] = None) -> Optional[dict]:
        if entity_type and entity_type in NODE_TYPES:
            query = f'MATCH (n:{entity_type} {{name: $name}}) RETURN n'
        else:
            query = 'MATCH (n {name: $name}) RETURN n LIMIT 1'

        with self._driver.session(database=self._database) as session:
            result = session.run(query, name=name)
            record = result.single()
            if record:
                node = record['n']
                return {
                    'name': node.get('name', ''),
                    'type': list(node.labels)[0] if node.labels else '',
                    'properties': dict(node),
                }
        return None

    def get_neighbors(self, name: str, relation: Optional[str] = None, depth: int = 1) -> GraphResult:
        rel_pattern = f'[r:{relation}]' if relation and relation in EDGE_TYPES else '[r]'
        query = (
            f'MATCH (n {{name: $name}})-{rel_pattern}-(m) '
            f'RETURN n, type(r) AS relation, r AS rel_props, m'
        )

        with self._driver.session(database=self._database) as session:
            result = session.run(query, name=name)
            records = [dict(r) for r in result]

        entities = []
        relationships = []
        seen = set()
        for rec in records:
            m = rec.get('m')
            if m and m.get('name') not in seen:
                seen.add(m.get('name'))
                entities.append({
                    'name': m.get('name', ''),
                    'type': list(m.labels)[0] if m.labels else '',
                    'properties': dict(m),
                })
            relationships.append({
                'source': name,
                'relation': rec.get('relation', ''),
                'target': m.get('name', '') if m else '',
                'properties': dict(rec.get('rel_props', {})),
            })

        return GraphResult(entities=entities, relationships=relationships, raw_records=records)

    def get_subgraph(self, name: str, depth: int = 2) -> GraphResult:
        query = (
            f'MATCH path = (n {{name: $name}})-[*1..{depth}]-(m) '
            f'UNWIND relationships(path) AS r '
            f'UNWIND nodes(path) AS node '
            f'RETURN DISTINCT node, startNode(r).name AS src, type(r) AS rel, endNode(r).name AS tgt'
        )

        with self._driver.session(database=self._database) as session:
            result = session.run(query, name=name)
            records = [dict(r) for r in result]

        entities = []
        relationships = []
        seen_entities = set()
        seen_rels = set()

        for rec in records:
            node = rec.get('node')
            if node and node.get('name') not in seen_entities:
                seen_entities.add(node.get('name'))
                entities.append({
                    'name': node.get('name', ''),
                    'type': list(node.labels)[0] if node.labels else '',
                    'properties': dict(node),
                })

            rel_key = (rec.get('src', ''), rec.get('rel', ''), rec.get('tgt', ''))
            if rel_key not in seen_rels:
                seen_rels.add(rel_key)
                relationships.append({
                    'source': rec.get('src', ''),
                    'relation': rec.get('rel', ''),
                    'target': rec.get('tgt', ''),
                })

        return GraphResult(entities=entities, relationships=relationships, raw_records=records)

    def search_entities(self, query_text: str, entity_type: Optional[str] = None, limit: int = 10) -> list[dict]:
        if entity_type and entity_type in NODE_TYPES:
            cypher = (
                f'MATCH (n:{entity_type}) '
                f'WHERE toLower(n.name) CONTAINS toLower($search_term) '
                f'RETURN n ORDER BY n.updated_at DESC LIMIT $limit'
            )
        else:
            cypher = (
                'MATCH (n) '
                'WHERE toLower(n.name) CONTAINS toLower($search_term) '
                'RETURN n ORDER BY n.updated_at DESC LIMIT $limit'
            )

        with self._driver.session(database=self._database) as session:
            result = session.run(cypher, search_term=query_text, limit=limit)
            return [
                {
                    'name': r['n'].get('name', ''),
                    'type': list(r['n'].labels)[0] if r['n'].labels else '',
                    'properties': dict(r['n']),
                }
                for r in result
            ]

    def delete_entity(self, name: str, entity_type: Optional[str] = None) -> bool:
        if entity_type and entity_type in NODE_TYPES:
            query = f'MATCH (n:{entity_type} {{name: $name}}) DETACH DELETE n RETURN count(n) AS deleted'
        else:
            query = 'MATCH (n {name: $name}) DETACH DELETE n RETURN count(n) AS deleted'

        with self._driver.session(database=self._database) as session:
            result = session.run(query, name=name)
            record = result.single()
            return record and record['deleted'] > 0

    def clear(self) -> None:
        with self._driver.session(database=self._database) as session:
            session.run('MATCH (n) DETACH DELETE n')
        logger.warning('Cleared entire relational memory graph.')

    def stats(self) -> dict:
        with self._driver.session(database=self._database) as session:
            node_result = session.run('MATCH (n) RETURN count(n) AS count')
            node_count = node_result.single()['count']

            rel_result = session.run('MATCH ()-[r]->() RETURN count(r) AS count')
            rel_count = rel_result.single()['count']

            label_result = session.run(
                'MATCH (n) RETURN labels(n)[0] AS label, count(n) AS count ORDER BY count DESC'
            )
            by_type = {r['label']: r['count'] for r in label_result}

        return {'total_nodes': node_count, 'total_relationships': rel_count, 'nodes_by_type': by_type}
