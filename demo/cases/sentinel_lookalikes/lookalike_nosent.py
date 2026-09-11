def f():
    # @not a sentinel
    s = """
#@+node:not-a-node
# @others
"""
    return s
