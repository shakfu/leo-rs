# @+leo-ver=5-thin
# @+node:corpus.20260911000000.1: * @file lookalike.py
# @+others
# @+node:corpus.20260911000000.2: ** f
def f():
    # @verbatim
    # @not a sentinel
    s = """
# @verbatim
#@+node:not-a-node
# @verbatim
# @others
"""
    return s
# @-others
# @-leo
