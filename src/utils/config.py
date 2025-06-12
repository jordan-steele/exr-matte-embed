import os
import json
from appdirs import user_config_dir

class Config:
    def __init__(self, app_name="EXRProcessor", app_author="EXRTools"):
        self.config_dir = user_config_dir(app_name, app_author)
        self.config_file = os.path.join(self.config_dir, 'config.json')
        os.makedirs(self.config_dir, exist_ok=True)
    
    def load(self):
        try:
            with open(self.config_file, 'r') as f:
                return json.load(f)
        except (FileNotFoundError, json.JSONDecodeError):
            return self.get_defaults()
    
    def save(self, config_data):
        try:
            with open(self.config_file, 'w') as f:
                json.dump(config_data, f)
            return True
        except Exception as e:
            print(f"Error saving config: {e}")
            return False
    
    @staticmethod
    def get_defaults():
        return {
            'matte_channel_name': 'matte',
            'last_folder_path': '',
            'compression': 'piz',
            'replace_originals': False
        }