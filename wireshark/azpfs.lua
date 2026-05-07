-- AZPFS Wireshark Dissector
-- Install dissector: symlink or copy azpfs.lua to ~/.local/lib/wireshark/plugins/
-- Install colors:    View > Coloring Rules > Import → select wireshark/colorfilters

local azpfs = Proto.new("azpfs", "AZPFS Protocol")

----------------------------------------
-- Value-string tables
----------------------------------------

local msg_type_names = {
    [0x00] = "ERROR",
    [0x01] = "INIT_REQ",
    [0x02] = "INIT_RES",
    [0x03] = "LOOKUP_REQ",
    [0x04] = "LOOKUP_RES",
    [0x05] = "GET_ATTR_REQ",
    [0x06] = "FILE_ATTR_RES",
    [0x07] = "SET_ATTR_REQ",
    [0x08] = "SUCCESS_RES",
    [0x09] = "STATS_REQ",
    [0x0A] = "STATS_RES",
    [0x0B] = "CREATE_REQ",
    [0x0C] = "READ_REQ",
    [0x0D] = "READ_RES",
    [0x0E] = "WRITE_REQ",
    [0x0F] = "READDIR_REQ",
    [0x10] = "RM_REQ",
    [0x11] = "MOVE_REQ",
}

local error_code_names = {
    [0x00] = "E_INTERNAL",
    [0x01] = "E_INVALID",
    [0x02] = "E_NOTFOUND",
    [0x03] = "E_EXISTS",
    [0x04] = "E_UNSUPPORTED",
}

local file_type_names = {
    [0] = "Named Pipe",
    [1] = "Char Device",
    [2] = "Block Device",
    [3] = "Directory",
    [4] = "Regular File",
    [5] = "Symlink",
    [6] = "Socket",
}

local request_types = {
    [0x01] = true, [0x03] = true, [0x05] = true, [0x07] = true,
    [0x09] = true, [0x0B] = true, [0x0C] = true, [0x0E] = true,
    [0x0F] = true, [0x10] = true, [0x11] = true,
}

----------------------------------------
-- ProtoFields
----------------------------------------

-- Common
local f_msg_type    = ProtoField.uint8("azpfs.msg_type", "Message Type", base.HEX, msg_type_names)
local f_request_id  = ProtoField.uint32("azpfs.request_id", "Request ID", base.DEC)

-- Request/response linking
local f_request_in  = ProtoField.framenum("azpfs.request_in", "Request in Frame", base.NONE, frametype.REQUEST)
local f_response_in = ProtoField.framenum("azpfs.response_in", "Response in Frame", base.NONE, frametype.RESPONSE)

-- ERROR
local f_error_code  = ProtoField.uint8("azpfs.error_code", "Error Code", base.HEX, error_code_names)
local f_error_msg_len = ProtoField.uint16("azpfs.error_msg_len", "Message Length", base.DEC)
local f_error_msg   = ProtoField.string("azpfs.error_msg", "Error Message")

-- INIT_REQ
local f_version     = ProtoField.uint8("azpfs.version", "Version", base.HEX)

-- INIT_RES
local f_accepted    = ProtoField.bool("azpfs.accepted", "Accepted", 8, nil, 0x80)

-- LOOKUP_REQ
local f_dir_inode   = ProtoField.uint64("azpfs.dir_inode", "Directory Inode", base.DEC)
local f_filename_len = ProtoField.uint8("azpfs.filename_len", "Filename Length", base.DEC)
local f_filename    = ProtoField.string("azpfs.filename", "Filename")

-- LOOKUP_RES
local f_inode       = ProtoField.uint64("azpfs.inode", "Inode", base.DEC)

-- FILE_ATTR_RES
local f_file_type   = ProtoField.uint8("azpfs.file_type", "File Type", base.HEX, file_type_names)
local f_size        = ProtoField.uint64("azpfs.size", "Size", base.DEC)
local f_blocks      = ProtoField.uint64("azpfs.blocks", "Blocks", base.DEC)
local f_atime       = ProtoField.uint64("azpfs.atime", "Access Time", base.DEC)
local f_mtime       = ProtoField.uint64("azpfs.mtime", "Modification Time", base.DEC)
local f_ctime       = ProtoField.uint64("azpfs.ctime", "Change Time", base.DEC)
local f_permissions = ProtoField.uint16("azpfs.permissions", "Permissions", base.OCT)
local f_nlinks      = ProtoField.uint32("azpfs.nlinks", "Hard Links", base.DEC)
local f_uid         = ProtoField.uint32("azpfs.uid", "UID", base.DEC)
local f_gid         = ProtoField.uint32("azpfs.gid", "GID", base.DEC)
local f_rdev        = ProtoField.uint32("azpfs.rdev", "rdev", base.DEC)
local f_blksize     = ProtoField.uint32("azpfs.blksize", "Block Size", base.DEC)

-- SET_ATTR_REQ
local f_field_mask  = ProtoField.uint8("azpfs.field_mask", "Field Mask", base.HEX)

-- STATS_RES
local f_stat_blksize  = ProtoField.uint32("azpfs.stat_blksize", "Block Size", base.DEC)
local f_stat_blocks   = ProtoField.uint64("azpfs.stat_blocks", "Total Blocks", base.DEC)
local f_free_blocks   = ProtoField.uint64("azpfs.free_blocks", "Free Blocks", base.DEC)
local f_avail_blocks  = ProtoField.uint64("azpfs.avail_blocks", "Available Blocks", base.DEC)
local f_total_inodes  = ProtoField.uint64("azpfs.total_inodes", "Total Inodes", base.DEC)
local f_free_inodes   = ProtoField.uint64("azpfs.free_inodes", "Free Inodes", base.DEC)
local f_max_fname_len = ProtoField.uint32("azpfs.max_filename_len", "Max Filename Length", base.DEC)
local f_frag_size     = ProtoField.uint32("azpfs.fragment_size", "Fragment Size", base.DEC)

-- CREATE_REQ
local f_unix_flags  = ProtoField.uint32("azpfs.unix_flags", "Unix Flags", base.HEX)
local f_is_directory = ProtoField.bool("azpfs.is_directory", "Directory", 8, nil, 0x80)

-- READ_REQ
local f_read_offset = ProtoField.uint64("azpfs.read_offset", "Offset", base.DEC)
local f_read_length = ProtoField.uint64("azpfs.read_length", "Length", base.DEC)

-- READ_RES
local f_total_length  = ProtoField.uint64("azpfs.total_length", "Total Length", base.DEC)
local f_chunk_length  = ProtoField.uint16("azpfs.chunk_length", "Chunk Length", base.DEC)
local f_chunk_offset  = ProtoField.uint64("azpfs.chunk_offset", "Chunk Offset", base.DEC)
local f_data          = ProtoField.bytes("azpfs.data", "Data")

-- WRITE_REQ
local f_write_offset  = ProtoField.uint64("azpfs.write_offset", "Offset", base.DEC)
local f_write_length  = ProtoField.uint32("azpfs.write_length", "Length", base.DEC)
local f_write_data    = ProtoField.bytes("azpfs.write_data", "Data")

-- MOVE_REQ
local f_dest_dir_inode    = ProtoField.uint64("azpfs.dest_dir_inode", "Destination Dir Inode", base.DEC)
local f_dest_filename_len = ProtoField.uint8("azpfs.dest_filename_len", "Destination Filename Length", base.DEC)
local f_dest_filename     = ProtoField.string("azpfs.dest_filename", "Destination Filename")

-- Reassembly
local f_reassembled       = ProtoField.bytes("azpfs.reassembled_data", "Reassembled Data")
local f_chunk_count       = ProtoField.uint32("azpfs.chunk_count", "Chunk Count", base.DEC)
local f_reassembled_in    = ProtoField.framenum("azpfs.reassembled_in", "Reassembled in Frame")

-- Dir entries
local f_de_inode      = ProtoField.uint64("azpfs.dir_entry.inode", "Inode", base.DEC)
local f_de_file_type  = ProtoField.uint8("azpfs.dir_entry.file_type", "File Type", base.HEX, file_type_names)
local f_de_fname_len  = ProtoField.uint8("azpfs.dir_entry.filename_len", "Filename Length", base.DEC)
local f_de_filename   = ProtoField.string("azpfs.dir_entry.filename", "Filename")

azpfs.fields = {
    f_msg_type, f_request_id, f_request_in, f_response_in,
    f_error_code, f_error_msg_len, f_error_msg,
    f_version, f_accepted,
    f_dir_inode, f_filename_len, f_filename, f_inode,
    f_file_type, f_size, f_blocks, f_atime, f_mtime, f_ctime,
    f_permissions, f_nlinks, f_uid, f_gid, f_rdev, f_blksize,
    f_field_mask,
    f_stat_blksize, f_stat_blocks, f_free_blocks, f_avail_blocks,
    f_total_inodes, f_free_inodes, f_max_fname_len, f_frag_size,
    f_unix_flags, f_is_directory,
    f_read_offset, f_read_length,
    f_total_length, f_chunk_length, f_chunk_offset, f_data,
    f_write_offset, f_write_length, f_write_data,
    f_dest_dir_inode, f_dest_filename_len, f_dest_filename,
    f_reassembled, f_chunk_count, f_reassembled_in,
    f_de_inode, f_de_file_type, f_de_fname_len, f_de_filename,
}

-- TCP stream field extractor (must be defined at script level)
local f_tcp_stream = Field.new("tcp.stream")

----------------------------------------
-- Preferences
----------------------------------------

azpfs.prefs.port = Pref.uint("TCP Port", 9000, "TCP port for AZPFS traffic")

----------------------------------------
-- State tables (populated on first pass only)
----------------------------------------

-- Request/response matching: keyed by "stream:request_id"
local req_frames = {}   -- key -> { frame=N, msg_type=T }
local resp_frames = {}  -- key -> frame number

-- Chunk reassembly: keyed by "stream:request_id"
local chunk_state = {}

----------------------------------------
-- Message length computation
----------------------------------------

-- Returns (total_message_length, nil) on success,
-- or (nil, bytes_needed) if the tvb is too short.
local function get_message_length(tvb, offset)
    local remaining = tvb:len() - offset
    if remaining < 1 then return nil, 1 end

    local msg_type = tvb(offset, 1):uint()

    -- Fixed-size messages
    local fixed = {
        [0x01] = 6,   -- INIT_REQ
        [0x02] = 6,   -- INIT_RES
        [0x04] = 13,  -- LOOKUP_RES
        [0x05] = 13,  -- GET_ATTR_REQ
        [0x06] = 68,  -- FILE_ATTR_RES
        [0x07] = 48,  -- SET_ATTR_REQ
        [0x08] = 5,   -- SUCCESS_RES
        [0x09] = 5,   -- STATS_REQ
        [0x0A] = 57,  -- STATS_RES
        [0x0C] = 29,  -- READ_REQ
        [0x0F] = 13,  -- READDIR_REQ
        [0x10] = 13,  -- RM_REQ
    }

    if fixed[msg_type] then
        if remaining < fixed[msg_type] then
            return nil, fixed[msg_type]
        end
        return fixed[msg_type], nil
    end

    -- Variable-size messages: need enough header to read the length field
    if msg_type == 0x00 then -- ERROR: header=8, + message_len (u16 at offset+6)
        if remaining < 8 then return nil, 8 end
        local msg_len = tvb(offset + 6, 2):uint()
        return 8 + msg_len, nil

    elseif msg_type == 0x03 then -- LOOKUP_REQ: header=14, + filename_len (u8 at offset+13)
        if remaining < 14 then return nil, 14 end
        local fname_len = tvb(offset + 13, 1):uint()
        return 14 + fname_len, nil

    elseif msg_type == 0x0B then -- CREATE_REQ: header=21, + filename_len (u8 at offset+20)
        if remaining < 21 then return nil, 21 end
        local fname_len = tvb(offset + 20, 1):uint()
        return 21 + fname_len, nil

    elseif msg_type == 0x0D then -- READ_RES: header=23, + chunk_len (u16 at offset+13)
        if remaining < 23 then return nil, 23 end
        local chunk_len = tvb(offset + 13, 2):uint()
        return 23 + chunk_len, nil

    elseif msg_type == 0x0E then -- WRITE_REQ: header=25, + length (u32 at offset+21)
        if remaining < 25 then return nil, 25 end
        local data_len = tvb(offset + 21, 4):uint()
        return 25 + data_len, nil

    elseif msg_type == 0x11 then -- MOVE_REQ: header=22, + dest_filename_len (u8 at offset+21)
        if remaining < 22 then return nil, 22 end
        local fname_len = tvb(offset + 21, 1):uint()
        return 22 + fname_len, nil
    end

    -- Unknown type
    return nil, nil
end

----------------------------------------
-- State key helper
----------------------------------------

local function state_key(pinfo, request_id)
    local stream = f_tcp_stream()
    local stream_id = stream and tostring(stream.value) or tostring(pinfo.number)
    return stream_id .. ":" .. tostring(request_id)
end

----------------------------------------
-- Directory entry parser
----------------------------------------

local function dissect_dir_entries(data_tvb, parent_tree)
    local offset = 0
    local len = data_tvb:len()
    local idx = 0
    while offset + 10 <= len do  -- minimum entry: 8 + 1 + 1 + 0
        local entry_inode = data_tvb(offset, 8):uint64()
        local entry_ftype = data_tvb(offset + 8, 1):uint()
        local entry_fname_len = data_tvb(offset + 9, 1):uint()
        if offset + 10 + entry_fname_len > len then break end
        local entry_fname = data_tvb(offset + 10, entry_fname_len):string()

        local entry_len = 10 + entry_fname_len
        local entry_tree = parent_tree:add(azpfs, data_tvb(offset, entry_len),
            string.format('Dir Entry: "%s" (inode %s, %s)',
                entry_fname,
                tostring(entry_inode),
                file_type_names[entry_ftype] or "Unknown"))
        entry_tree:add(f_de_inode, data_tvb(offset, 8))
        entry_tree:add(f_de_file_type, data_tvb(offset + 8, 1))
        entry_tree:add(f_de_fname_len, data_tvb(offset + 9, 1))
        entry_tree:add(f_de_filename, data_tvb(offset + 10, entry_fname_len))

        offset = offset + entry_len
        idx = idx + 1
    end
    return idx
end

----------------------------------------
-- Chunk reassembly display
----------------------------------------

local function handle_chunk_reassembly(tvb, offset, msg_len, pinfo, subtree, request_id, chunk_off, chunk_len, total_len)
    local key = state_key(pinfo, request_id)

    if not pinfo.visited then
        if not chunk_state[key] then
            -- Determine request type from matching request
            local req_type = nil
            local req_info = req_frames[key]
            if req_info then req_type = req_info.msg_type end

            chunk_state[key] = {
                total_length = total_len,
                request_type = req_type,
                chunks = {},
                accumulated = 0,
                reassembled_in = nil,
                data_parts = {},
            }
        end

        local cs = chunk_state[key]
        if not cs.chunks[chunk_off] then
            cs.chunks[chunk_off] = { frame = pinfo.number, len = chunk_len }
            cs.accumulated = cs.accumulated + chunk_len
            -- Store raw bytes for reassembly
            cs.data_parts[chunk_off] = tvb(offset + 23, chunk_len):bytes()
        end
        if cs.accumulated >= cs.total_length and not cs.reassembled_in then
            cs.reassembled_in = pinfo.number
        end
    end

    local cs = chunk_state[key]
    if not cs then return end

    -- Count chunks
    local num_chunks = 0
    for _ in pairs(cs.chunks) do num_chunks = num_chunks + 1 end

    if cs.reassembled_in == pinfo.number then
        -- This is the frame where reassembly completed
        local reasm_tree = subtree:add(azpfs, tvb(offset, msg_len),
            string.format("[Reassembled AZPFS Data (%d bytes, %d chunks)]",
                cs.total_length, num_chunks))
        reasm_tree:add(f_chunk_count, num_chunks):set_generated(true)

        -- Reassemble data in offset order
        local offsets = {}
        for off in pairs(cs.data_parts) do offsets[#offsets + 1] = off end
        table.sort(offsets)

        local reassembled = ByteArray.new()
        for _, off in ipairs(offsets) do
            reassembled:append(cs.data_parts[off])
        end

        if reassembled:len() > 0 then
            local reasm_tvb = reassembled:tvb("Reassembled AZPFS Data")
            reasm_tree:add(f_reassembled, reasm_tvb())

            -- Parse dir entries if this was a READDIR response
            if cs.request_type == 0x0F then
                local de_tree = reasm_tree:add(azpfs, reasm_tvb(),
                    "[Directory Entries]")
                local count = dissect_dir_entries(reasm_tvb, de_tree)
                de_tree:append_text(string.format(" (%d entries)", count))
            end
        end
    elseif cs.reassembled_in then
        -- Not the final frame — add a pointer
        subtree:add(f_reassembled_in, cs.reassembled_in):set_generated(true)
        subtree:append_text(string.format(" [Reassembled in frame %d]", cs.reassembled_in))
    end
end

----------------------------------------
-- Per-message dissection
----------------------------------------

local function dissect_message(tvb, offset, msg_len, pinfo, tree)
    local msg_type = tvb(offset, 1):uint()
    local name = msg_type_names[msg_type] or "UNKNOWN"

    local subtree = tree:add(azpfs, tvb(offset, msg_len), "AZPFS " .. name)
    subtree:add(f_msg_type, tvb(offset, 1))

    local request_id = nil
    if msg_len >= 5 then
        request_id = tvb(offset + 1, 4):uint()
        subtree:add(f_request_id, tvb(offset + 1, 4))
    end

    -- Request/response linking
    if request_id then
        local key = state_key(pinfo, request_id)
        local is_request = request_types[msg_type]

        if not pinfo.visited then
            if is_request then
                req_frames[key] = { frame = pinfo.number, msg_type = msg_type }
            else
                resp_frames[key] = pinfo.number
            end
        end

        if is_request then
            if resp_frames[key] then
                subtree:add(f_response_in, resp_frames[key]):set_generated(true)
            end
        else
            local req_info = req_frames[key]
            if req_info then
                subtree:add(f_request_in, req_info.frame):set_generated(true)
            end
        end
    end

    local info = name

    -- Type-specific fields
    if msg_type == 0x00 then -- ERROR
        subtree:add(f_error_code, tvb(offset + 5, 1))
        subtree:add(f_error_msg_len, tvb(offset + 6, 2))
        local elen = tvb(offset + 6, 2):uint()
        if elen > 0 then
            subtree:add(f_error_msg, tvb(offset + 8, elen))
            info = string.format("ERROR %s: %s",
                error_code_names[tvb(offset + 5, 1):uint()] or "?",
                tvb(offset + 8, elen):string())
        else
            info = string.format("ERROR %s",
                error_code_names[tvb(offset + 5, 1):uint()] or "?")
        end

    elseif msg_type == 0x01 then -- INIT_REQ
        local vbyte = tvb(offset + 5, 1):uint()
        local ver = bit.rshift(vbyte, 4)
        subtree:add(f_version, tvb(offset + 5, 1), ver)
        info = string.format("INIT_REQ version=%d", ver)

    elseif msg_type == 0x02 then -- INIT_RES
        subtree:add(f_accepted, tvb(offset + 5, 1))
        local acc = bit.rshift(tvb(offset + 5, 1):uint(), 7) ~= 0
        info = string.format("INIT_RES accepted=%s", tostring(acc))

    elseif msg_type == 0x03 then -- LOOKUP_REQ
        subtree:add(f_dir_inode, tvb(offset + 5, 8))
        subtree:add(f_filename_len, tvb(offset + 13, 1))
        local flen = tvb(offset + 13, 1):uint()
        if flen > 0 then
            subtree:add(f_filename, tvb(offset + 14, flen))
            info = string.format("LOOKUP_REQ dir=%s '%s'",
                tostring(tvb(offset + 5, 8):uint64()),
                tvb(offset + 14, flen):string())
        end

    elseif msg_type == 0x04 then -- LOOKUP_RES
        subtree:add(f_inode, tvb(offset + 5, 8))
        info = string.format("LOOKUP_RES inode=%s", tostring(tvb(offset + 5, 8):uint64()))

    elseif msg_type == 0x05 then -- GET_ATTR_REQ
        subtree:add(f_inode, tvb(offset + 5, 8))
        info = string.format("GET_ATTR_REQ inode=%s", tostring(tvb(offset + 5, 8):uint64()))

    elseif msg_type == 0x06 then -- FILE_ATTR_RES
        local type_byte = tvb(offset + 5, 1):uint()
        local ftype = bit.rshift(type_byte, 5)
        subtree:add(f_file_type, tvb(offset + 5, 1), ftype)
        subtree:add(f_size, tvb(offset + 6, 8))
        subtree:add(f_blocks, tvb(offset + 14, 8))
        subtree:add(f_atime, tvb(offset + 22, 8))
        subtree:add(f_mtime, tvb(offset + 30, 8))
        subtree:add(f_ctime, tvb(offset + 38, 8))
        subtree:add(f_permissions, tvb(offset + 46, 2))
        subtree:add(f_nlinks, tvb(offset + 48, 4))
        subtree:add(f_uid, tvb(offset + 52, 4))
        subtree:add(f_gid, tvb(offset + 56, 4))
        subtree:add(f_rdev, tvb(offset + 60, 4))
        subtree:add(f_blksize, tvb(offset + 64, 4))
        info = string.format("FILE_ATTR_RES %s size=%s",
            file_type_names[ftype] or "?",
            tostring(tvb(offset + 6, 8):uint64()))

    elseif msg_type == 0x07 then -- SET_ATTR_REQ
        subtree:add(f_inode, tvb(offset + 5, 8))
        local mask_byte = tvb(offset + 13, 1):uint()
        local mask = bit.rshift(mask_byte, 2)
        subtree:add(f_field_mask, tvb(offset + 13, 1), mask)
        subtree:add(f_size, tvb(offset + 14, 8))
        subtree:add(f_atime, tvb(offset + 22, 8))
        subtree:add(f_mtime, tvb(offset + 30, 8))
        subtree:add(f_permissions, tvb(offset + 38, 2))
        subtree:add(f_uid, tvb(offset + 40, 4))
        subtree:add(f_gid, tvb(offset + 44, 4))

        local parts = {}
        if bit.band(mask, 0x01) ~= 0 then parts[#parts+1] = "size" end
        if bit.band(mask, 0x02) ~= 0 then parts[#parts+1] = "atime" end
        if bit.band(mask, 0x04) ~= 0 then parts[#parts+1] = "mtime" end
        if bit.band(mask, 0x08) ~= 0 then parts[#parts+1] = "perms" end
        if bit.band(mask, 0x10) ~= 0 then parts[#parts+1] = "uid" end
        if bit.band(mask, 0x20) ~= 0 then parts[#parts+1] = "gid" end
        info = string.format("SET_ATTR_REQ inode=%s [%s]",
            tostring(tvb(offset + 5, 8):uint64()),
            table.concat(parts, ","))

    elseif msg_type == 0x08 then -- SUCCESS_RES
        info = "SUCCESS_RES"

    elseif msg_type == 0x09 then -- STATS_REQ
        info = "STATS_REQ"

    elseif msg_type == 0x0A then -- STATS_RES
        subtree:add(f_stat_blksize, tvb(offset + 5, 4))
        subtree:add(f_stat_blocks, tvb(offset + 9, 8))
        subtree:add(f_free_blocks, tvb(offset + 17, 8))
        subtree:add(f_avail_blocks, tvb(offset + 25, 8))
        subtree:add(f_total_inodes, tvb(offset + 33, 8))
        subtree:add(f_free_inodes, tvb(offset + 41, 8))
        subtree:add(f_max_fname_len, tvb(offset + 49, 4))
        subtree:add(f_frag_size, tvb(offset + 53, 4))
        info = "STATS_RES"

    elseif msg_type == 0x0B then -- CREATE_REQ
        subtree:add(f_dir_inode, tvb(offset + 5, 8))
        subtree:add(f_permissions, tvb(offset + 13, 2))
        subtree:add(f_unix_flags, tvb(offset + 15, 4))
        subtree:add(f_is_directory, tvb(offset + 19, 1))
        subtree:add(f_filename_len, tvb(offset + 20, 1))
        local flen = tvb(offset + 20, 1):uint()
        if flen > 0 then
            subtree:add(f_filename, tvb(offset + 21, flen))
        end
        local is_dir = bit.rshift(tvb(offset + 19, 1):uint(), 7) ~= 0
        info = string.format("CREATE_REQ %s '%s' in dir=%s",
            is_dir and "mkdir" or "create",
            flen > 0 and tvb(offset + 21, flen):string() or "",
            tostring(tvb(offset + 5, 8):uint64()))

    elseif msg_type == 0x0C then -- READ_REQ
        subtree:add(f_inode, tvb(offset + 5, 8))
        subtree:add(f_read_offset, tvb(offset + 13, 8))
        subtree:add(f_read_length, tvb(offset + 21, 8))
        info = string.format("READ_REQ inode=%s offset=%s len=%s",
            tostring(tvb(offset + 5, 8):uint64()),
            tostring(tvb(offset + 13, 8):uint64()),
            tostring(tvb(offset + 21, 8):uint64()))

    elseif msg_type == 0x0D then -- READ_RES
        subtree:add(f_total_length, tvb(offset + 5, 8))
        local chunk_len = tvb(offset + 13, 2):uint()
        subtree:add(f_chunk_length, tvb(offset + 13, 2))
        subtree:add(f_chunk_offset, tvb(offset + 15, 8))
        if chunk_len > 0 then
            subtree:add(f_data, tvb(offset + 23, chunk_len))
        end

        local total_len = tvb(offset + 5, 8):uint64():tonumber()
        local chunk_off = tvb(offset + 15, 8):uint64():tonumber()

        info = string.format("READ_RES chunk %d-%d/%d",
            chunk_off, chunk_off + chunk_len - 1,
            total_len)

        -- Chunk reassembly
        if request_id then
            handle_chunk_reassembly(tvb, offset, msg_len, pinfo, subtree,
                request_id, chunk_off, chunk_len, total_len)
        end

    elseif msg_type == 0x0E then -- WRITE_REQ
        subtree:add(f_inode, tvb(offset + 5, 8))
        subtree:add(f_write_offset, tvb(offset + 13, 8))
        subtree:add(f_write_length, tvb(offset + 21, 4))
        local dlen = tvb(offset + 21, 4):uint()
        if dlen > 0 then
            subtree:add(f_write_data, tvb(offset + 25, dlen))
        end
        info = string.format("WRITE_REQ inode=%s offset=%s len=%d",
            tostring(tvb(offset + 5, 8):uint64()),
            tostring(tvb(offset + 13, 8):uint64()),
            dlen)

    elseif msg_type == 0x0F then -- READDIR_REQ
        subtree:add(f_inode, tvb(offset + 5, 8))
        info = string.format("READDIR_REQ inode=%s", tostring(tvb(offset + 5, 8):uint64()))

    elseif msg_type == 0x10 then -- RM_REQ
        subtree:add(f_inode, tvb(offset + 5, 8))
        info = string.format("RM_REQ inode=%s", tostring(tvb(offset + 5, 8):uint64()))

    elseif msg_type == 0x11 then -- MOVE_REQ
        subtree:add(f_inode, tvb(offset + 5, 8))
        subtree:add(f_dest_dir_inode, tvb(offset + 13, 8))
        subtree:add(f_dest_filename_len, tvb(offset + 21, 1))
        local flen = tvb(offset + 21, 1):uint()
        if flen > 0 then
            subtree:add(f_dest_filename, tvb(offset + 22, flen))
            info = string.format("MOVE_REQ inode=%s -> dir=%s/'%s'",
                tostring(tvb(offset + 5, 8):uint64()),
                tostring(tvb(offset + 13, 8):uint64()),
                tvb(offset + 22, flen):string())
        end
    end

    -- Add request ID to info column
    if request_id then
        info = info .. string.format(" [id=%d]", request_id)
    end

    return info
end

----------------------------------------
-- Main dissector function
----------------------------------------

function azpfs.dissector(tvb, pinfo, tree)
    local len = tvb:len()
    if len == 0 then return 0 end

    local offset = 0
    local messages_found = 0
    local infos = {}

    while offset < len do
        local msg_len, needed = get_message_length(tvb, offset)

        if msg_len == nil then
            if needed == nil then
                -- Unknown message type — bail
                return 0
            end
            -- Need more data
            pinfo.desegment_len = needed - (len - offset)
            pinfo.desegment_offset = offset
            return offset
        end

        if len - offset < msg_len then
            pinfo.desegment_len = msg_len - (len - offset)
            pinfo.desegment_offset = offset
            return offset
        end

        -- Dissect this message
        local info = dissect_message(tvb, offset, msg_len, pinfo, tree)
        infos[#infos + 1] = info
        messages_found = messages_found + 1
        offset = offset + msg_len
    end

    if messages_found > 0 then
        pinfo.cols.protocol = "AZPFS"
        pinfo.cols.info = table.concat(infos, " | ")
    end

    return offset
end

----------------------------------------
-- Registration
----------------------------------------

local function register_port(port)
    DissectorTable.get("tcp.port"):add(port, azpfs)
end

register_port(azpfs.prefs.port)

function azpfs.prefs_changed()
    -- Re-register when preference changes
    DissectorTable.get("tcp.port"):remove_all(azpfs)
    register_port(azpfs.prefs.port)
end
