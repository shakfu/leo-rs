# @+leo-ver=5-thin
# @+node:corpus.20260911000000.1: * @file clones.py
# @+others
# @+node:corpus.20260911000000.2: ** shared
def shared():
    return 1
# @+node:corpus.20260911000000.3: ** other
def other():
    # @+others
    # @+node:corpus.20260911000000.2: *3* shared
    def shared():
        return 1
    # @-others
    return 2
# @-others
# @-leo
