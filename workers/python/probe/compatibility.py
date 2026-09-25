"""Six fixed synthetic compatibility assertions; imports occur only in execute."""
import importlib
import importlib.metadata
from pathlib import Path
import re


def require(condition):
    if not condition:
        raise ValueError('fixed-compatibility-assertion')


def distribution_versions(site, wanted):
    observed = {}
    distributions = list(importlib.metadata.distributions(path=[str(site)]))
    require(len(distributions) <= 128)
    for distribution in distributions:
        name = re.sub(r'[-_.]+', '-', distribution.metadata['Name']).lower()
        if name in wanted:
            require(name not in observed)
            observed[name] = distribution.version
    require(observed == wanted)
    return observed, distributions


def write_transactions(pa, pq, path, rows):
    require(len(rows) <= 16)
    table = pa.table({name: pa.array([row[name] for row in rows], pa.string())
                      for name in ('id', 'amount', 'currency', 'review', 'transfer_peer')})
    pq.write_table(table, path, compression='NONE', use_dictionary=False)
    require(path.stat().st_size <= 1024 * 1024)


def execute(fixture, prefix, job, checkpoint, import_checkpoint):
    require(fixture['schema_version'] == 1)
    site = prefix / 'install/lib/python3.13/site-packages'
    checkpoint('versions')
    versions, distributions = distribution_versions(site, fixture['versions'])
    checkpoint('imports')
    imported = ['duckdb', 'networkx', 'spacy', 'click', 'splink', 'pyarrow']
    modules = {}
    for name in imported:
        import_checkpoint(name, 'before')
        modules[name] = importlib.import_module(name)
        import_checkpoint(name, 'after')
    for module in modules.values():
        require(Path(module.__file__).resolve().is_relative_to(site))

    checkpoint('mentions')
    spacy = modules['spacy']
    from spacy.matcher import PhraseMatcher
    nlp = spacy.blank('en')
    require(nlp.lang == 'en')
    matcher = PhraseMatcher(nlp.vocab, attr='LOWER')
    matcher.add('selected_identifiers', [nlp.make_doc(value) for value in fixture['mentions']['terms']])
    doc = nlp.make_doc(fixture['mentions']['text'])
    matches = [{'text': doc[start:end].text, 'start': doc[start].idx, 'end': doc[end - 1].idx + len(doc[end - 1]),
                'review': 'pending'} for _, start, end in matcher(doc)]
    phrase_empty = len(matcher(nlp.make_doc(fixture['mentions']['empty_text']))) == 0

    checkpoint('graph')
    nx = modules['networkx']
    graph = nx.Graph()
    for edge in fixture['graph']['edges']:
        if edge['review'] == 'accepted':
            graph.add_edge(edge['subject'], edge['object'], assertion_id=edge['id'])
    path = nx.shortest_path(graph, fixture['graph']['source'], fixture['graph']['target'], backend='networkx')
    assertions = [graph[a][b]['assertion_id'] for a, b in zip(path, path[1:])]
    unreachable = False
    try:
        nx.shortest_path(graph, fixture['graph']['source'], fixture['graph']['unreachable'], backend='networkx')
    except (nx.NetworkXNoPath, nx.NodeNotFound):
        unreachable = True

    checkpoint('transactions')
    pa = modules['pyarrow']
    import pyarrow.parquet as pq
    from transaction_totals import transaction_totals
    from decimal import localcontext
    pa.set_cpu_count(1)
    pa.set_io_thread_count(1)
    source = job / 'scratch/transactions.parquet'
    write_transactions(pa, pq, source, fixture['transactions'])
    with localcontext() as context:
        context.prec = 2  # Decimal input construction and exact aggregation must survive this.
        totals = transaction_totals(source)
    rejected = False
    transfer = job / 'scratch/transfer.parquet'
    write_transactions(pa, pq, transfer, [{'id': 'transfer', 'amount': '-1', 'currency': 'AUD',
                                         'review': 'accepted', 'transfer_peer': 'unverified-peer'}])
    try:
        transaction_totals(transfer)
    except ValueError as error:
        rejected = str(error) == 'Transfer selection requires canonical pair validation'

    checkpoint('plugins')
    observed = {name: {} for name in fixture['entry_point_groups']}
    for distribution in distributions:
        for entry in distribution.entry_points:
            if entry.group in observed:
                require(entry.name not in observed[entry.group])
                observed[entry.group][entry.name] = entry.value
    metadata_matched = observed == fixture['entry_point_groups']
    from spacy.util import registry
    architecture = registry.get('architectures', 'spacy-legacy.Tok2Vec.v1')
    layer = registry.get('layers', 'spacy-legacy.StaticVectors.v1')
    reader = registry.get('readers', 'srsly.read_json.v1')
    require(callable(architecture) and callable(layer) and callable(reader))
    registry_result = {'metadata_matched': metadata_matched,
                       'architecture': architecture.__module__ + ':' + architecture.__name__,
                       'layer': layer.__module__ + ':' + layer.__name__,
                       'reader': reader(job / 'input/reader.json')}
    # The parent validates every returned value against compiled expected fixtures.
    # Splink is deliberately imported only: no uncalibrated matching is performed.
    return {'versions': versions, 'imported_modules': imported, 'phrase_matches': matches, 'phrase_empty': phrase_empty,
            'graph_path': path, 'graph_assertions': assertions, 'graph_unreachable': unreachable,
            'transaction_totals': totals, 'transfer_rejected': rejected, 'registry': registry_result}
