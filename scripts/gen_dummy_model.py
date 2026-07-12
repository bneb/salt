import struct
import random
import string

# =============================================================================
# Model Config
# =============================================================================
dim = 64
hidden_dim = 128
n_layers = 2
n_heads = 4
n_kv_heads = 4
vocab_size = 256
seq_len = 32

def write_model(filename):
    with open(filename, 'wb') as f:
        # Config Header (7 x i32)
        f.write(struct.pack('7i', dim, hidden_dim, n_layers, n_heads, n_kv_heads, vocab_size, seq_len))
        
        head_size = dim // n_heads
        
        sizes = [
            vocab_size * dim,             # token_embedding_table
            n_layers * dim,               # rms_att_weight
            n_layers * dim * (n_heads * head_size),    # wq
            n_layers * dim * (n_kv_heads * head_size), # wk
            n_layers * dim * (n_kv_heads * head_size), # wv
            n_layers * (n_heads * head_size) * dim,    # wo
            n_layers * dim,               # rms_ffn_weight
            n_layers * dim * hidden_dim,  # w1
            n_layers * hidden_dim * dim,  # w2
            n_layers * dim * hidden_dim,  # w3
            dim,                          # rms_final_weight
        ]
        total = sum(sizes)
        print(f"Model: {total} params")
        
        chunk = 1024
        remaining = total
        while remaining > 0:
            batch = min(remaining, chunk)
            floats = [random.uniform(-0.1, 0.1) for _ in range(batch)]
            f.write(struct.pack(f'{batch}f', *floats))
            remaining -= batch
        
    print(f"Wrote {filename}")

def write_tokenizer(filename):
    """Generate a tokenizer.bin compatible with llama2.c format."""
    with open(filename, 'wb') as f:
        # max_token_length
        max_len = 16
        f.write(struct.pack('i', max_len))
        
        # 256 vocab entries (byte-level: each byte gets its own token)
        for i in range(vocab_size):
            if i < 128 and chr(i).isprintable() and not chr(i).isspace():
                text = chr(i).encode('utf-8')
            elif i == 10:  # newline
                text = b'\n'
            elif i == 32:  # space
                text = b' '
            else:
                # Non-printable: use hex representation
                text = f'<{i:02x}>'.encode('utf-8')
            
            score = float(i)  # Simple scores
            f.write(struct.pack('f', score))  # score
            f.write(struct.pack('i', len(text)))  # len
            f.write(text)  # bytes
    
    print(f"Wrote {filename} ({vocab_size} entries)")

if __name__ == "__main__":
    write_model("dummy.bin")
    write_tokenizer("tokenizer.bin")
