/*
 * Copyright (c) 2026 contributors.
 *
 * The MIT License
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in
 * all copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
 * THE SOFTWARE.
 */
#pragma once

#include <cstdint>
#include <string>

namespace graphene { namespace witness_node {

/**
 * Keeps other witness_node processes off this data directory while this one runs.
 *
 * Two nodes on one data directory corrupt its database. The lock is an operating system lock on
 * <data dir>/witness_node.lock (LockFileEx on Windows, flock elsewhere), so it goes away with the process,
 * even when the process crashes or is killed: there is never a stale lock to clean up by hand.
 */
class data_dir_lock
{
public:
   data_dir_lock() = default;
   ~data_dir_lock();
   data_dir_lock( const data_dir_lock& ) = delete;
   data_dir_lock& operator=( const data_dir_lock& ) = delete;

   /// Takes the lock, creating the directory if needed. On failure returns false and fills `error` with a
   /// message for the user that names the holder's PID when it is known.
   bool acquire( const std::string& data_dir, std::string& error );

private:
#ifdef _WIN32
   void* _file = nullptr;
#else
   int _fd = -1;
#endif
};

/// Exit code of a node that found its data directory in use, so a supervisor can tell it from a crash
constexpr int EXIT_DATA_DIR_IN_USE = 3;

} } // graphene::witness_node
