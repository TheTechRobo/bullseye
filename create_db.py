#!/usr/bin/env python3

from rethinkdb import r
import os

db = lambda : r.db("atuploads")
table = lambda : db().table("uploads")
conn = r.connect(host = os.getenv("RETHINKDB_HOST", "127.0.0.1"))

if "atuploads" not in r.db_list().run(conn):
    print(r.db_create("atuploads").run(conn))
if "uploads" not in db().table_list().run(conn):
    print(db().table_create("uploads").run(conn))
if "nf_status" not in table().index_list().run(conn):
    print(table().index_create("nf_status", [r.row['project'], r.row['pipeline'], r.row['status'], r.row['processing']]).run(conn))
