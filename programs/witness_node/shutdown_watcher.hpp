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

#ifdef _WIN32

#include <cstdint>
#include <functional>
#include <stdexcept>
#include <string>
#include <thread>

namespace graphene { namespace witness_node {

/**
 * Lets a GUI that runs the node without a console stop it cleanly on Windows.
 *
 * Ctrl+C cannot reach a process that shares no console with the sender, and TerminateProcess leaves the
 * database dirty. Instead the node waits on a named event the GUI signals (--shutdown-event) and on the
 * GUI process itself (--parent-pid), so the node also exits if the GUI dies without signalling.
 */
class shutdown_watcher
{
public:
   /// Opens the event and the process up front; throws std::runtime_error if either cannot be opened.
   /// An empty event_name or a zero parent_pid skips that source.
   shutdown_watcher( const std::string& event_name, uint32_t parent_pid );
   ~shutdown_watcher();

   shutdown_watcher( const shutdown_watcher& ) = delete;
   shutdown_watcher& operator=( const shutdown_watcher& ) = delete;

   /// Calls on_shutdown at most once, from a background thread, with the reason the node should exit.
   void start( std::function<void(const std::string&)> on_shutdown );

private:
   void* _stop = nullptr;   // unnamed event, set by the destructor to end the wait
   void* _event = nullptr;
   void* _parent = nullptr;
   uint32_t _parent_pid = 0;
   std::string _event_name;
   std::thread _thread;
};

} } // graphene::witness_node

#endif // _WIN32
